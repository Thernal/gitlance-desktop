; Python's roles after the grammar's own query (a later pattern wins).
((identifier) @role.constant
  (#match? @role.constant "^[A-Z][A-Z0-9_]+$"))
(function_definition name: (identifier) @role.function)
(class_definition name: (identifier) @role.type)
(call function: (identifier) @role.function-call)
(call function: (attribute attribute: (identifier) @role.function-call))
(parameters (identifier) @role.parameter)
(default_parameter name: (identifier) @role.parameter)
(typed_parameter (identifier) @role.parameter)
(typed_default_parameter name: (identifier) @role.parameter)
(keyword_argument name: (identifier) @role.named-argument)
(attribute attribute: (identifier) @role.field)
(decorator) @role.annotation
