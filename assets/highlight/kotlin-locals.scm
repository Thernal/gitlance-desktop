; Kotlin's scopes, so a parameter or a local keeps its role where it is used.
(function_declaration) @local.scope
(lambda_literal) @local.scope
(anonymous_function) @local.scope
(secondary_constructor) @local.scope

(parameter (identifier) @local.definition)
(lambda_parameters (variable_declaration (identifier) @local.definition))
(catch_block (identifier) @local.definition)
(property_declaration (variable_declaration (identifier) @local.definition))
(class_parameter (identifier) @local.definition)

(identifier) @local.reference
