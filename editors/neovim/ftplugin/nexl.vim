" Nexl filetype plugin
if exists('b:did_ftplugin')
  finish
endif
let b:did_ftplugin = 1

setlocal commentstring=;\ %s
setlocal shiftwidth=2
setlocal softtabstop=2
setlocal expandtab
setlocal lisp
setlocal iskeyword+=!,?,*,+,-,/,<,=,>,&,.

let b:undo_ftplugin = 'setlocal commentstring< shiftwidth< softtabstop< expandtab< lisp< iskeyword<'
