; Go's roles after the grammar's own query (a later pattern wins).
(function_declaration name: (identifier) @role.function)
(method_declaration name: (field_identifier) @role.function)
(call_expression function: (identifier) @role.function-call)
(call_expression function: (selector_expression field: (field_identifier) @role.function-call))
(parameter_declaration name: (identifier) @role.parameter)
(field_identifier) @role.field
(short_var_declaration left: (expression_list (identifier) @role.local))
