//! Atoms Lucerna needs, interned once per connection.

use x11rb::atom_manager;

atom_manager! {
    pub Atoms: AtomsCookie {
        UTF8_STRING,
        STRING,
        CARDINAL,
        ATOM,
        WINDOW,
        WM_CLASS,
        WM_NAME,
        WM_CLIENT_MACHINE,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_DESKTOP,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_BELOW,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_BYPASS_COMPOSITOR,
        _NET_SUPPORTING_WM_CHECK,
        _NET_SUPPORTED,
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_RESTACK_WINDOW,
        _MOTIF_WM_HINTS,
        _LUCERNA_WALLPAPER,
        _LUCERNA_QUIT,
    }
}
