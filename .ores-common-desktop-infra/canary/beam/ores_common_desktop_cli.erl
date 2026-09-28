-module(ores_common_desktop_cli).

-export([request/3, request/4, allowed_command/1]).

request(RequestId, ProductId, Command) when is_binary(RequestId), is_binary(ProductId), is_atom(Command) ->
    request(RequestId, ProductId, Command, undefined).

request(RequestId, ProductId, Command, Payload) when is_binary(RequestId), is_binary(ProductId), is_atom(Command) ->
    case allowed_command(Command) of
        true ->
            Base = #{request_id => RequestId, product_id => ProductId, command => Command},
            case Payload of undefined -> {ok, Base}; _ -> {ok, Base#{payload => Payload}} end;
        false -> {error, unsupported_command}
    end.

allowed_command(doctor) -> true;
allowed_command(status) -> true;
allowed_command(plan) -> true;
allowed_command(up) -> true;
allowed_command(down) -> true;
allowed_command(reload) -> true;
allowed_command(update) -> true;
allowed_command(rollback) -> true;
allowed_command(routes) -> true;
allowed_command(logs) -> true;
allowed_command(deploy) -> true;
allowed_command(_) -> false.
