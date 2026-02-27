" Nexl syntax highlighting (regex-based fallback)
" For full highlighting, use tree-sitter-nexl

if exists('b:current_syntax')
  finish
endif

" Comments
syntax match nexlComment ";.*$" contains=nexlTodo
syntax keyword nexlTodo TODO FIXME NOTE HACK XXX contained

" Strings
syntax region nexlString start=/"/ skip=/\\\\\|\\"/ end=/"/ contains=nexlStringEscape
syntax match nexlStringEscape /\\[nrt\\"0]/ contained
syntax match nexlStringEscape /\\u{[0-9a-fA-F]\+}/ contained

" Character literals
syntax match nexlChar /\\[a-zA-Z0-9]/
syntax match nexlChar /\\space/
syntax match nexlChar /\\newline/
syntax match nexlChar /\\tab/
syntax match nexlChar /\\return/
syntax match nexlChar /\\u{[0-9a-fA-F]\+}/

" Numbers
syntax match nexlNumber /\v<-?\d+(\.\d+)?([eE][+-]?\d+)?([fi]\d+)?>/
syntax match nexlNumber /\v<-?\d+\/\d+>/
syntax match nexlNumber /\v<0[xX][0-9a-fA-F]+>/
syntax match nexlNumber /\v<0[bB][01]+>/
syntax match nexlNumber /\v<0[oO][0-7]+>/

" Keywords (atoms starting with :)
syntax match nexlKeyword /:\v[a-zA-Z!?*+\-/<=>&.][a-zA-Z0-9!?*+\-/<=>&._]*/

" Booleans and unit
syntax keyword nexlBoolean true false
syntax keyword nexlConstant unit

" Special forms
syntax keyword nexlSpecialForm def defn fn let do if cond match when unless
syntax keyword nexlSpecialForm deftype defeffect defprotocol defmacro
syntax keyword nexlSpecialForm handle import module try for each times loop
syntax keyword nexlSpecialForm quote quasiquote unquote unquote-splice
syntax keyword nexlSpecialForm defextern defexport defpattern impl
syntax keyword nexlSpecialForm recur set! new

" Type names (capitalized identifiers)
syntax match nexlType /\v<[A-Z][a-zA-Z0-9]*/

" Discard macro
syntax match nexlDiscard /#_/

" Set literal opener
syntax match nexlSetOpen /#{/

" Deref
syntax match nexlDeref /@\v[a-zA-Z!?*+\-/<=>&.][a-zA-Z0-9!?*+\-/<=>&._]*/

" Highlighting links
highlight default link nexlComment Comment
highlight default link nexlTodo Todo
highlight default link nexlString String
highlight default link nexlStringEscape SpecialChar
highlight default link nexlChar Character
highlight default link nexlNumber Number
highlight default link nexlKeyword Constant
highlight default link nexlBoolean Boolean
highlight default link nexlConstant Constant
highlight default link nexlSpecialForm Keyword
highlight default link nexlType Type
highlight default link nexlDiscard Comment
highlight default link nexlSetOpen Delimiter
highlight default link nexlDeref Special

let b:current_syntax = 'nexl'
