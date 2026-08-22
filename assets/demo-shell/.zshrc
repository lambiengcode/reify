# Shell configuration for the recorded demo. Kept here, and pointed at with ZDOTDIR,
# so the recording never depends on whoever runs it having a particular prompt,
# plugin set, or aliases in their own ~/.zshrc.

# Dracula's green, matching the theme vhs renders the terminal in.
PROMPT='%F{#50fa7b}❯%f '
RPROMPT=''

# Green the command word as it is typed. A real shell with syntax highlighting shows
# a valid command in green, and that is worth having in the demo - but installing a
# plugin to get it would make the recording depend on the recorder's machine. This is
# the same effect in a few lines: highlight the first word when it resolves to
# something executable, which is exactly what the plugin is signalling.
_demo_highlight() {
  region_highlight=()
  local word=${BUFFER%% *}
  if [[ -n $word ]] && (( $+commands[$word] )); then
    region_highlight=("0 ${#word} fg=#50fa7b")
  fi
}
zle -N zle-line-pre-redraw _demo_highlight

# No history file, no completion cache: nothing the demo does should touch the
# recorder's own shell state.
unset HISTFILE
setopt no_beep
