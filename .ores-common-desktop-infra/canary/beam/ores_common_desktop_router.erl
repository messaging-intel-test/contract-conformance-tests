-module(ores_common_desktop_router).
-behaviour(gen_server).

-export([start_link/0, replace_routes/2, route/2, revision/0]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2, code_change/3]).

-define(SERVER, ?MODULE).
-define(SNAPSHOT_KEY, {?MODULE, snapshot}).

start_link() ->
    gen_server:start_link({local, ?SERVER}, ?MODULE, [], []).

replace_routes(Revision, Routes) when is_binary(Revision), is_map(Routes) ->
    gen_server:call(?SERVER, {replace_routes, Revision, Routes}).

route(Host, Path) when is_binary(Host), is_binary(Path) ->
    Snapshot = persistent_term:get(?SNAPSHOT_KEY, empty_snapshot()),
    Routes = maps:get(routes, Snapshot),
    lookup_route(Host, Path, Routes).

revision() ->
    Snapshot = persistent_term:get(?SNAPSHOT_KEY, empty_snapshot()),
    maps:get(revision, Snapshot).

init([]) ->
    persistent_term:put(?SNAPSHOT_KEY, empty_snapshot()),
    {ok, #{}}.

handle_call({replace_routes, Revision, Routes}, _From, State) ->
    ok = validate_revision(Revision),
    ok = validate_routes(Routes),
    Snapshot = #{
        revision => Revision,
        routes => Routes
    },
    persistent_term:put(?SNAPSHOT_KEY, Snapshot),
    {reply, ok, State};
handle_call(_Request, _From, State) ->
    {reply, {error, unsupported_request}, State}.

handle_cast(_Message, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, _State) ->
    ok.

code_change(_OldVersion, State, _Extra) ->
    {ok, State}.

empty_snapshot() ->
    #{revision => undefined, routes => #{}}.

validate_revision(Revision) when byte_size(Revision) > 0 ->
    ok;
validate_revision(_Revision) ->
    error(invalid_revision).

validate_routes(Routes) ->
    maps:fold(
        fun(Key, Target, ok) ->
            validate_route(Key, Target)
        end,
        ok,
        Routes
    ).

validate_route({Host, Prefix}, #{host := TargetHost, port := Port})
        when is_binary(Host), byte_size(Host) > 0,
             is_binary(Prefix), byte_size(Prefix) > 0,
             is_integer(Port), Port > 0, Port < 65536 ->
    ok = validate_path_prefix(Prefix),
    ok = validate_loopback(TargetHost),
    ok;
validate_route(_Key, _Target) ->
    error(invalid_route).

validate_path_prefix(<<"/", _/binary>>) ->
    ok;
validate_path_prefix(_Prefix) ->
    error(invalid_route_prefix).

validate_loopback({127, _, _, _}) ->
    ok;
validate_loopback({0, 0, 0, 0, 0, 0, 0, 1}) ->
    ok;
validate_loopback(_TargetHost) ->
    error(non_loopback_route_target).

lookup_route(Host, Path, Routes) ->
    Candidates = [
        {byte_size(Prefix), Target}
        || {{RouteHost, Prefix}, Target} <- maps:to_list(Routes),
           RouteHost =:= Host,
           has_prefix(Path, Prefix)
    ],
    case lists:reverse(lists:keysort(1, Candidates)) of
        [{_Length, Target} | _] ->
            {ok, Target};
        [] ->
            not_found
    end.

has_prefix(Value, Prefix) ->
    PrefixSize = byte_size(Prefix),
    case Value of
        <<Prefix:PrefixSize/binary, _/binary>> ->
            true;
        _ ->
            false
    end.
