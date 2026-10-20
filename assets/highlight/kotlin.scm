; Kotlin's roles (tree-sitter-kotlin-ng ships no queries), as Design's highlighting spec names them.
; A later pattern that captures the same node wins: the general ones come first, the particular last.

; Every name, so a use takes its definition's role from the locals query.
(identifier) @role.local

; Keywords.
[
  "abstract" "actual" "annotation" "as" "by" "catch" "class" "companion" "const" "constructor"
  "crossinline" "data" "do" "else" "enum" "expect" "external" "final" "finally" "for" "fun" "get"
  "if" "import" "in" "infix" "init" "inline" "inner" "interface" "internal" "is" "lateinit"
  "noinline" "object" "open" "operator" "out" "override" "package" "private" "protected" "public"
  "return" "return@" "sealed" "set" "super" "super@" "suspend" "tailrec" "this" "this@" "throw"
  "try" "typealias" "val" "value" "var" "vararg" "when" "where" "while"
] @keyword

; Operators.
[
  "!" "!!" "!=" "!==" "!in" "!is" "%" "%=" "&&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "->" ".."
  "..<" "/" "/=" "::" "<" "<=" "=" "==" "===" ">" ">=" "?:" "?." "as?" "||"
] @operator

; Literals.
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string
(escape_sequence) @escape
(interpolation "${" @escape "}" @escape)
(number_literal) @number
(float_literal) @number

; A member read after a dot.
(navigation_expression (identifier) @role.field .)

; A property in a function is a local; one of a class or a file a field.
(property_declaration (variable_declaration (identifier) @role.local))
(property_declaration (multi_variable_declaration (variable_declaration (identifier) @role.local)))
(class_body (property_declaration (variable_declaration (identifier) @role.field)))
(source_file (property_declaration (variable_declaration (identifier) @role.field)))

; Parameters; a constructor's properties are fields.
(parameter (identifier) @role.parameter)
(lambda_parameters (variable_declaration (identifier) @role.parameter))
(lambda_parameters (multi_variable_declaration (variable_declaration (identifier) @role.parameter)))
(catch_block (identifier) @role.parameter)
(class_parameter (identifier) @role.field)

; A named argument: `to = …`.
(value_argument (identifier) @role.named-argument . "=")

; Calls: the name called, plain or after a dot.
(call_expression . (identifier) @role.function-call)
(call_expression . (navigation_expression (identifier) @role.function-call .))

; Types, and a capitalised name used as one (a constructor call, a companion's members).
((identifier) @role.type
  (#match? @role.type "^[A-Z]"))
(user_type (identifier) @role.type)

; Annotations.
(annotation "@" @role.annotation)
(annotation (user_type (identifier) @role.annotation))
(annotation (constructor_invocation (user_type (identifier) @role.annotation)))
(label) @role.annotation

; What a name declares.
(class_declaration name: (identifier) @role.type)
(object_declaration name: (identifier) @role.type)
(companion_object name: (identifier) @role.type)
(function_declaration name: (identifier) @role.function)

; SCREAMING_CASE is a constant; an enum entry too.
((identifier) @role.constant
  (#match? @role.constant "^[A-Z][A-Z0-9_]+$"))
(enum_entry (identifier) @role.constant)

; Literals this grammar reads as names.
((identifier) @keyword
  (#any-of? @keyword "true" "false" "null"))
