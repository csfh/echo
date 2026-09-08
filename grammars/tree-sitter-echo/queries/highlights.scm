; Echo highlights — generated from echo_syntax leader set.
; Captures: leaders vs idents vs literals vs comments.

; Line comments (; → EOL)
(comment) @comment

; Leader-only tokens (named rules)
(leader_tilde) @keyword
(leader_dollar) @keyword
(leader_hash) @keyword
(leader_at) @keyword
(leader_question) @keyword
(leader_caret) @keyword
(leader_backslash) @keyword

; Dual-use leaders as statement introducers (anonymous glyph in context)
; leader_percent (%)
; leader_colon (:)
; leader_bang (!)
; leader_star (*)
; leader_lt (<)
; leader_gt (>)
; leader_pipe (|)
; leader_plus (+)
; leader_minus (-)
; leader_slash (/)
(struct_statement leader: "%" @keyword)
(match_arm leader: "%" @keyword)
(else_if_statement leader: ":" @keyword)
(else_statement leader: ":" @keyword)
(match_arm leader: ":" @keyword)
(error_return_statement leader: "!" @keyword)
(match_arm leader: "!" @keyword)
(loop_statement leader: "*" @keyword)
(break_statement leader: "<" @keyword)
(continue_statement leader: ">" @keyword)
(match_leader) @keyword
(task_spawn_statement leader: "+" @keyword)
(task_join_statement leader: "-" @keyword)
(import_statement leader: "/" @keyword)

; Identifiers
(ident) @variable
(bind_clause target: (bind_lhs) @variable)
(bind_lhs (ident) @variable)
(bind_lhs field: (ident) @property)
(struct_statement name: (ident) @type)
(struct_extend_statement name: (ident) @type)
(struct_literal type: (ident) @type)
(struct_literal type: (field_expression) @type)
(field_initializer name: (ident) @property)
(field_expression field: (ident) @property)
(call_expression function: (ident) @function)
(call_expression function: (field_expression field: (ident) @function.method))
(export_statement names: (ident) @variable)
(import_statement path: (import_path) @namespace)

; Literals
(number) @number
(duration) @number
(string_pure) @string
(string_rich) @string
(bytes_pure) @string
(bytes_rich) @string
(locator_pure) @string
(locator_rich) @string
; true/false atoms (expr `|` / `_`); match leader `|` highlighted above
(true_atom) @constant.builtin
(false_atom) @constant.builtin
(receiver) @variable.builtin
(self_field field: (ident) @property)
(width_cast type: (ident) @type)

; Operators / punctuation (expression dual-use surface)
; Listed before more-specific leader captures win via query order in editors
; that last-match-wins; leaders above already mark statement glyphs.
[
"=" "==" "!=" "===" "!=="
"<" ">" "<=" ">="
"+" "-" "*" "/" "%"
"&&" "||" "!" ".."
"." "," ":"
"(" ")" "[" "]" "{" "}"
] @operator
