; Java's roles after the grammar's own query (a later pattern wins): what it does not tell apart.
((identifier) @role.constant
  (#match? @role.constant "^[A-Z][A-Z0-9_]+$"))
(method_declaration name: (identifier) @role.function)
(constructor_declaration name: (identifier) @role.function)
(method_invocation name: (identifier) @role.function-call)
(formal_parameter name: (identifier) @role.parameter)
(catch_formal_parameter name: (identifier) @role.parameter)
(field_declaration declarator: (variable_declarator name: (identifier) @role.field))
(field_access field: (identifier) @role.field)
(local_variable_declaration declarator: (variable_declarator name: (identifier) @role.local))
(annotation name: (identifier) @role.annotation)
(marker_annotation name: (identifier) @role.annotation)
