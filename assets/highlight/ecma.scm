; JavaScript's and TypeScript's roles after the grammars' own queries (a later pattern wins).
(function_declaration name: (identifier) @role.function)
(generator_function_declaration name: (identifier) @role.function)
(method_definition name: (property_identifier) @role.function)
(call_expression function: (identifier) @role.function-call)
(call_expression function: (member_expression property: (property_identifier) @role.function-call))
(variable_declarator name: (identifier) @role.local)
