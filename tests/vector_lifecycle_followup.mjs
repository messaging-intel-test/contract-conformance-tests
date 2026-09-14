#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const pin = JSON.parse(readFileSync(path.join(root, "vector-source-pin.json"), "utf8"));
const productRoot = process.env.PRODUCT_ROOT;
if (!productRoot) throw new Error("PRODUCT_ROOT is required");
if (!/^[0-9a-f]{40}$/.test(pin.revision)) throw new Error("product revision must be an exact SHA");
if (!/^[a-z][a-z0-9_]*$/.test(pin.schema) || !/^[a-z][a-z0-9_]*$/.test(pin.purpose)) {
  throw new Error("unsafe schema or purpose in vector pin");
}

const productPath = (relative) => path.join(productRoot, relative);
const manifest = JSON.parse(readFileSync(productPath("embedding-contract/generation.json"), "utf8"));
const database = manifest.databaseFirst ?? {};
const desired = database.postgresDesiredState ?? database.postgres;
const preflight = database.postgresFamilyPreflight;
const reconcile = database.postgresIndexReconciliation;
const adapter = database.supabasePrivateAdapter;
for (const relative of [desired, preflight, reconcile, adapter]) {
  if (!relative || !existsSync(productPath(relative))) throw new Error(`missing product artifact: ${relative}`);
}

const env = { ...process.env, PGCONNECT_TIMEOUT: "10" };
const psql = (args, input = undefined, allowFailure = false) => {
  const result = spawnSync("psql", ["-X", "--set", "ON_ERROR_STOP=1", ...args], {
    env,
    encoding: "utf8",
    input,
  });
  if (!allowFailure && result.status !== 0) {
    throw new Error(`psql failed (${args.join(" ")}):\n${result.stdout}${result.stderr}`);
  }
  return result;
};
const run = (sql) => psql(["--quiet", "--command", sql]).stdout.trim();
const scalar = (sql) => psql(["--tuples-only", "--no-align", "--quiet", "--command", sql]).stdout.trim();
const runFile = (relative) => psql(["--quiet", "--file", productPath(relative)]);
const mustFail = (sql) => {
  const result = psql(["--quiet", "--command", sql], undefined, true);
  return result.status !== 0;
};

const results = [];
const check = (name, condition, detail = "") => {
  if (!condition) throw new Error(`FAILED ${name}${detail ? `: ${detail}` : ""}`);
  results.push(name);
  process.stdout.write(`ok ${String(results.length).padStart(2, "0")} - ${name}\n`);
};

run(`
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='anon') THEN CREATE ROLE anon NOLOGIN; END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='authenticated') THEN CREATE ROLE authenticated NOLOGIN; END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='service_role') THEN CREATE ROLE service_role NOLOGIN; END IF;
END
$$;
`);
runFile(desired);
runFile(preflight);
runFile(reconcile);
runFile(adapter);

const schema = pin.schema;
const purpose = pin.purpose;
const exact = `${schema}.semantic_embeddings`;
const indexed = `${schema}.semantic_embedding_index`;
const triggerFunction = `${schema}.sync_semantic_embedding_index_4000()`;
const tenant = "11111111-1111-4111-8111-111111111111";
const fixture = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const repairFixture = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

check("projection trigger exists and is enabled",
  scalar(`SELECT count(*) FROM pg_trigger
    WHERE tgrelid='${exact}'::regclass
      AND tgname='semantic_embeddings_sync_index_4000'
      AND NOT tgisinternal AND tgenabled <> 'D'`) === "1");

check("projection trigger function is SECURITY INVOKER",
  scalar(`SELECT NOT p.prosecdef FROM pg_proc p
    JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='${schema}' AND p.proname='sync_semantic_embedding_index_4000'`) === "t");

check("projection trigger function pins an empty search_path",
  scalar(`SELECT coalesce(array_to_string(p.proconfig,'|'),'') LIKE '%search_path=""%'
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='${schema}' AND p.proname='sync_semantic_embedding_index_4000'`) === "t");

run(`INSERT INTO ${exact}
  (embedding_id,tenant_id,entity_kind,entity_id,purpose,embedding_provider,model,
   original_dimensions,embedding,normalization,content_hash,search_text,metadata)
VALUES
  ('${fixture}','${tenant}','lifecycle_test','fixture','${purpose}','openai','text-embedding-3-small',
   1536,${schema}.pad_embedding_4100(array_fill(0.01::real,ARRAY[1536])),
   'provider',repeat('1',64),'lifecycle fixture','{"phase":1}'::jsonb);`);

check("exact insert creates exactly one ANN projection",
  scalar(`SELECT count(*) FROM ${indexed} WHERE embedding_id='${fixture}'`) === "1");

const beforeProjection = scalar(`SELECT indexed_embedding::text FROM ${indexed} WHERE embedding_id='${fixture}'`);
run(`UPDATE ${exact}
  SET embedding=${schema}.pad_embedding_4100(array_fill(0.02::real,ARRAY[1536]))
  WHERE embedding_id='${fixture}';`);
const afterProjection = scalar(`SELECT indexed_embedding::text FROM ${indexed} WHERE embedding_id='${fixture}'`);

check("exact vector update refreshes the ANN projection",
  beforeProjection !== afterProjection && afterProjection.length > 0);

run(`UPDATE ${exact} SET metadata='{"phase":2,"note":"metadata-only"}'::jsonb WHERE embedding_id='${fixture}';`);

check("metadata-only update preserves one-to-one projection cardinality",
  scalar(`SELECT count(*) FROM ${indexed} WHERE embedding_id='${fixture}'`) === "1");

run(`DELETE FROM ${exact} WHERE embedding_id='${fixture}';`);

check("exact delete cascades to the ANN projection",
  scalar(`SELECT count(*) FROM ${indexed} WHERE embedding_id='${fixture}'`) === "0");

run(`INSERT INTO ${exact}
  (embedding_id,tenant_id,entity_kind,entity_id,purpose,embedding_provider,model,
   original_dimensions,embedding,normalization,content_hash,search_text)
VALUES
  ('${repairFixture}','${tenant}','lifecycle_test','repair-fixture','${purpose}','openai','text-embedding-3-small',
   1536,${schema}.pad_embedding_4100(array_fill(0.03::real,ARRAY[1536])),
   'provider',repeat('2',64),'reconciliation fixture');`);
run(`DELETE FROM ${indexed} WHERE embedding_id='${repairFixture}';`);
if (scalar(`SELECT count(*) FROM ${indexed} WHERE embedding_id='${repairFixture}'`) !== "0") {
  throw new Error("test setup failed to remove the projection before reconciliation");
}
runFile(reconcile);

check("reconciliation repairs a missing ANN projection",
  scalar(`SELECT count(*) FROM ${indexed} WHERE embedding_id='${repairFixture}'`) === "1");

const reconciliationFingerprint = scalar(`SELECT count(*) || '|' || md5(string_agg(embedding_id::text || ':' || indexed_embedding::text, ',' ORDER BY embedding_id)) FROM ${indexed}`);
runFile(reconcile);

check("reconciliation is idempotent on an already-converged index",
  scalar(`SELECT count(*) || '|' || md5(string_agg(embedding_id::text || ':' || indexed_embedding::text, ',' ORDER BY embedding_id)) FROM ${indexed}`) === reconciliationFingerprint);

check("non-zero padding tail is rejected",
  mustFail(`INSERT INTO ${exact}
    (tenant_id,entity_kind,entity_id,purpose,embedding_provider,model,original_dimensions,embedding,normalization,content_hash)
  VALUES
    ('${tenant}','lifecycle_test','bad-tail','${purpose}','openai','text-embedding-3-small',1536,
     (array_fill(0.01::real,ARRAY[1536]) || ARRAY[0.5::real] || array_fill(0.0::real,ARRAY[2563]))::extensions.vector(4100),
     'provider',repeat('3',64));`));

check("unknown embedding model is rejected by the model-profile foreign key",
  mustFail(`INSERT INTO ${exact}
    (tenant_id,entity_kind,entity_id,purpose,embedding_provider,model,original_dimensions,embedding,normalization,content_hash)
  VALUES
    ('${tenant}','lifecycle_test','bad-model','${purpose}','openai','not-a-real-embedding-model',1536,
     ${schema}.pad_embedding_4100(array_fill(0.01::real,ARRAY[1536])),'provider',repeat('4',64));`));

check("exact and ANN tables both FORCE row-level security",
  scalar(`SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='${schema}'
      AND c.relname IN ('semantic_embeddings','semantic_embedding_index')
      AND c.relrowsecurity AND c.relforcerowsecurity`) === "2");

check("anon has no USAGE on the private product schema",
  scalar(`SELECT has_schema_privilege('anon','${schema}','USAGE')`) === "f");

check("authenticated has no USAGE on the private product schema",
  scalar(`SELECT has_schema_privilege('authenticated','${schema}','USAGE')`) === "f");

check("service_role cannot directly execute the internal projection trigger function",
  scalar(`SELECT has_function_privilege('service_role','${triggerFunction}','EXECUTE')`) === "f");

if (results.length !== 15) throw new Error(`expected exactly 15 lifecycle invariants, ran ${results.length}`);
process.stdout.write(`vector lifecycle follow-up passed: ${results.length}/15 new invariants\n`);
