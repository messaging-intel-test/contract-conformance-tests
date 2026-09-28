-module(ores_common_desktop_upgrade).

-export([reload_modules/1, upgrade_server/4]).

reload_modules(Modules) when is_list(Modules) -> reload_modules(Modules, []).
upgrade_server(Server, Module, OldVersion, Extra) when is_atom(Module) ->
    case sys:suspend(Server) of
        ok ->
            try
                case load_module(Module) of
                    ok -> case sys:change_code(Server, Module, OldVersion, Extra) of ok -> ok; {error, Reason} -> {error, {code_change_failed, Module, Reason}} end;
                    {error, _Reason} = Error -> Error
                end
            after _ = sys:resume(Server) end;
        {error, Reason} -> {error, {suspend_failed, Server, Reason}}
    end.
reload_modules([], Loaded) -> {ok, lists:reverse(Loaded)};
reload_modules([Module | Rest], Loaded) when is_atom(Module) -> case load_module(Module) of ok -> reload_modules(Rest, [Module | Loaded]); {error, Reason} -> {error, {Module, Reason, lists:reverse(Loaded)}} end;
reload_modules([Invalid | _Rest], Loaded) -> {error, {invalid_module, Invalid, lists:reverse(Loaded)}}.
load_module(Module) -> case code:soft_purge(Module) of true -> case code:load_file(Module) of {module, Module} -> ok; {error, Reason} -> {error, {load_failed, Reason}} end; false -> {error, module_busy} end.
