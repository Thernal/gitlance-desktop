; Rust's roles after the grammar's own query (a later pattern wins).
((identifier) @role.constant
  (#match? @role.constant "^[A-Z][A-Z0-9_]+$"))
(function_item name: (identifier) @role.function)
(function_signature_item name: (identifier) @role.function)
(call_expression function: (identifier) @role.function-call)
(call_expression function: (field_expression field: (field_identifier) @role.function-call))
(call_expression function: (scoped_identifier name: (identifier) @role.function-call))
(generic_function function: (identifier) @role.function-call)
(macro_invocation macro: (identifier) @role.function-call)
(parameter pattern: (identifier) @role.parameter)
(closure_parameters (identifier) @role.parameter)
(field_identifier) @role.field
(shorthand_field_identifier) @role.field
(let_declaration pattern: (identifier) @role.local)
(attribute_item) @role.annotation
(inner_attribute_item) @role.annotation
