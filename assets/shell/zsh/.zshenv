# .zshenv de LordOS Terminal — se usa solo para arrancar: la terminal lanza zsh con
# ZDOTDIR apuntando a esta carpeta. Acá se restaura el ZDOTDIR del usuario, se carga su
# propio .zshenv y se registra la integración (lord-zsh.zsh). Después zsh sigue leyendo
# .zprofile / .zshrc / .zlogin del usuario como siempre.
if [[ -n "${LORD_ZDOTDIR+x}" ]]; then
    ZDOTDIR=$LORD_ZDOTDIR
    unset LORD_ZDOTDIR
else
    unset ZDOTDIR
fi
__lord_zsh_dir=${${(%):-%x}:A:h:h}
[[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]] && source "${ZDOTDIR:-$HOME}/.zshenv"
[[ -o interactive && -r "$__lord_zsh_dir/lord-zsh.zsh" ]] && source "$__lord_zsh_dir/lord-zsh.zsh"
unset __lord_zsh_dir
