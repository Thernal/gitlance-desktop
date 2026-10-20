(method_declaration) @local.scope
(constructor_declaration) @local.scope
(lambda_expression) @local.scope
(block) @local.scope
(formal_parameter name: (identifier) @local.definition)
(catch_formal_parameter name: (identifier) @local.definition)
(local_variable_declaration declarator: (variable_declarator name: (identifier) @local.definition))
(field_declaration declarator: (variable_declarator name: (identifier) @local.definition))
(identifier) @local.reference
