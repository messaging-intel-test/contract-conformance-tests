-module(ores_common_desktop_adapter).

-export([desired_state/1]).

-callback product_id() -> binary().
-callback desired_routes() -> map().
-callback desired_services() -> [map()].
-callback validate_product_state(map()) -> ok | {error, term()}.

-optional_callbacks([validate_product_state/1]).

desired_state(Adapter) when is_atom(Adapter) ->
    ProductId = Adapter:product_id(),
    Routes = Adapter:desired_routes(),
    Services = Adapter:desired_services(),
    State = #{product_id => ProductId, routes => Routes, services => Services},
    validate_adapter_state(Adapter, State).

validate_adapter_state(Adapter, State) ->
    case erlang:function_exported(Adapter, validate_product_state, 1) of
        true ->
            case Adapter:validate_product_state(State) of
                ok -> {ok, State};
                {error, _Reason} = Error -> Error
            end;
        false -> {ok, State}
    end.
