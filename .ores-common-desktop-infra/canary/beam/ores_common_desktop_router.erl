-module(ores_common_desktop_router).
-behaviour(gen_server).

-export([start_link/0, replace_routes/2, route/2, revision/0]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2, code_change/3]).

-define(SERVER, ?MODULE).
-define(ROUTES_KEY, {?MODULE, routes}).
-define(REVISION_KEY, {?MODULE, revision}).

start_link() -> gen_server:start_link({local, ?SERVER}, ?MODULE, [], []).
replace_routes(Revision, Routes) when is_binary(Revision), is_map(Routes) -> gen_server:call(?SERVER, {replace_routes, Revision, Routes}).
route(Host, Path) when is_binary(Host), is_binary(Path) -> Routes = persistent_term:get(?ROUTES_KEY, #{}), lookup_route(Host, Path, Routes).
revision() -> persistent_term:get(?REVISION_KEY, undefined).
init([]) -> persistent_term:put(?ROUTES_KEY, #{}), persistent_term:put(?REVISION_KEY, undefined), {ok, #{}}.
handle_call({replace_routes, Revision, Routes}, _From, State) -> ok = validate_routes(Routes), persistent_term:put(?ROUTES_KEY, Routes), persistent_term:put(?REVISION_KEY, Revision), {reply, ok, State};
handle_call(_Request, _From, State) -> {reply, {error, unsupported_request}, State}.
handle_cast(_Message, State) -> {noreply, State}.
handle_info(_Info, State) -> {noreply, State}.
terminate(_Reason, _State) -> ok.
code_change(_OldVersion, State, _Extra) -> {ok, State}.
validate_routes(Routes) -> maps:fold(fun(Key, Target, ok) -> validate_route(Key, Target) end, ok, Routes).
validate_route({Host, Prefix}, #{host := TargetHost, port := Port}) when is_binary(Host), is_binary(Prefix), is_tuple(TargetHost), is_integer(Port), Port > 0, Port < 65536 -> ok;
validate_route(_Key, _Target) -> error(invalid_route).
lookup_route(Host, Path, Routes) ->
    Candidates = [{byte_size(Prefix), Target} || {{RouteHost, Prefix}, Target} <- maps:to_list(Routes), RouteHost =:= Host, has_prefix(Path, Prefix)],
    case lists:reverse(lists:keysort(1, Candidates)) of [{_Length, Target} | _] -> {ok, Target}; [] -> not_found end.
has_prefix(Value, Prefix) -> PrefixSize = byte_size(Prefix), case Value of <<Prefix:PrefixSize/binary, _/binary>> -> true; _ -> false end.
