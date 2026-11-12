//! Physical key names, factory shortcuts and menu symbols. Order is significant.
use serde_json::Value;
use std::sync::OnceLock;
pub fn code_keys() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"{
  "2": "1",
  "3": "2",
  "4": "3",
  "5": "4",
  "6": "5",
  "7": "6",
  "8": "7",
  "9": "8",
  "10": "9",
  "11": "0",
  "12": "-",
  "13": "=",
  "16": "q",
  "17": "w",
  "18": "e",
  "19": "r",
  "20": "t",
  "21": "y",
  "22": "u",
  "23": "i",
  "24": "o",
  "25": "p",
  "26": "[",
  "27": "]",
  "30": "a",
  "31": "s",
  "32": "d",
  "33": "f",
  "34": "g",
  "35": "h",
  "36": "j",
  "37": "k",
  "38": "l",
  "39": ";",
  "40": "'",
  "41": "`",
  "43": "\\",
  "44": "z",
  "45": "x",
  "46": "c",
  "47": "v",
  "48": "b",
  "49": "n",
  "50": "m",
  "51": ",",
  "52": ".",
  "53": "/",
  "57": "Space"
}"##,
        )
        .expect("terminal keys codeKeys")
    })
}
pub fn bindings() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"[
  [
    "newTab",
    [
      "Super+t",
      "Ctrl+Shift+t"
    ]
  ],
  [
    "newWindow",
    [
      "Super+n",
      "Ctrl+Shift+n"
    ]
  ],
  [
    "closePane",
    [
      "Super+w",
      "Ctrl+Shift+w"
    ]
  ],
  [
    "closeTab",
    [
      "Super+Shift+w"
    ]
  ],
  [
    "splitRight",
    [
      "Super+d",
      "Ctrl+Shift+d"
    ]
  ],
  [
    "splitDown",
    [
      "Super+Shift+d",
      "Ctrl+Shift+e"
    ]
  ],
  [
    "nextPane",
    [
      "Super+]",
      "Ctrl+Alt+]"
    ]
  ],
  [
    "prevPane",
    [
      "Super+[",
      "Ctrl+Alt+["
    ]
  ],
  [
    "paneLeft",
    [
      "Super+Alt+ArrowLeft",
      "Ctrl+Alt+ArrowLeft"
    ]
  ],
  [
    "paneRight",
    [
      "Super+Alt+ArrowRight",
      "Ctrl+Alt+ArrowRight"
    ]
  ],
  [
    "paneUp",
    [
      "Super+Alt+ArrowUp",
      "Ctrl+Alt+ArrowUp"
    ]
  ],
  [
    "paneDown",
    [
      "Super+Alt+ArrowDown",
      "Ctrl+Alt+ArrowDown"
    ]
  ],
  [
    "nextTab",
    [
      "Super+Shift+]",
      "Ctrl+Tab",
      "Ctrl+PageDown"
    ]
  ],
  [
    "prevTab",
    [
      "Super+Shift+[",
      "Ctrl+Shift+Tab",
      "Ctrl+PageUp"
    ]
  ],
  [
    "find",
    [
      "Super+f",
      "Ctrl+Shift+f"
    ]
  ],
  [
    "findNext",
    [
      "Super+g"
    ]
  ],
  [
    "findPrev",
    [
      "Super+Shift+g"
    ]
  ],
  [
    "copy",
    [
      "Super+c",
      "Ctrl+Shift+c"
    ]
  ],
  [
    "paste",
    [
      "Super+v",
      "Ctrl+Shift+v",
      "Shift+Insert"
    ]
  ],
  [
    "pasteHistory",
    [
      "Super+Shift+h"
    ]
  ],
  [
    "selectAll",
    [
      "Super+a",
      "Ctrl+Shift+a"
    ]
  ],
  [
    "selectOutput",
    [
      "Super+Shift+a"
    ]
  ],
  [
    "clearBuffer",
    [
      "Super+k",
      "Ctrl+Shift+k"
    ]
  ],
  [
    "zoomIn",
    [
      "Super+=",
      "Super+Shift+=",
      "Ctrl+Shift+="
    ]
  ],
  [
    "zoomOut",
    [
      "Super+-",
      "Ctrl+Shift+-"
    ]
  ],
  [
    "zoomReset",
    [
      "Super+0",
      "Ctrl+Shift+0"
    ]
  ],
  [
    "maximizePane",
    [
      "Super+Shift+Enter"
    ]
  ],
  [
    "fullscreen",
    [
      "Super+Enter"
    ]
  ],
  [
    "prevMark",
    [
      "Super+Shift+ArrowUp"
    ]
  ],
  [
    "nextMark",
    [
      "Super+Shift+ArrowDown"
    ]
  ],
  [
    "setMark",
    [
      "Super+Shift+m"
    ]
  ],
  [
    "scrollTop",
    [
      "Super+Home",
      "Shift+Home"
    ]
  ],
  [
    "scrollBottom",
    [
      "Super+End",
      "Shift+End"
    ]
  ],
  [
    "scrollPageUp",
    [
      "Super+PageUp",
      "Shift+PageUp"
    ]
  ],
  [
    "scrollPageDown",
    [
      "Super+PageDown",
      "Shift+PageDown"
    ]
  ],
  [
    "scrollLineUp",
    [
      "Super+ArrowUp",
      "Ctrl+Shift+ArrowUp"
    ]
  ],
  [
    "scrollLineDown",
    [
      "Super+ArrowDown",
      "Ctrl+Shift+ArrowDown"
    ]
  ],
  [
    "broadcast",
    [
      "Super+Alt+i"
    ]
  ],
  [
    "autocomplete",
    [
      "Super+;"
    ]
  ],
  [
    "composer",
    [
      "Super+Shift+."
    ]
  ],
  [
    "openQuickly",
    [
      "Super+Shift+o"
    ]
  ],
  [
    "preferences",
    [
      "Super+,"
    ]
  ],
  [
    "toolbelt",
    [
      "Super+Shift+b"
    ]
  ],
  [
    "timestamps",
    [
      "Super+Shift+e"
    ]
  ],
  [
    "editTitle",
    [
      "Super+i"
    ]
  ],
  [
    "reset",
    [
      "Super+r"
    ]
  ],
  [
    "copyMode",
    [
      "Super+Shift+c"
    ]
  ],
  [
    "instantReplay",
    [
      "Super+Alt+b"
    ]
  ],
  [
    "commandHistory",
    [
      "Super+Shift+;"
    ]
  ],
  [
    "undoClose",
    [
      "Super+z"
    ]
  ],
  [
    "alertNextMark",
    [
      "Super+Alt+a"
    ]
  ],
  [
    "equalize",
    [
      "Super+Alt+="
    ]
  ],
  [
    "paneToTab",
    [
      "Super+Alt+t"
    ]
  ],
  [
    "moveTabLeft",
    [
      "Super+Shift+ArrowLeft"
    ]
  ],
  [
    "moveTabRight",
    [
      "Super+Shift+ArrowRight"
    ]
  ],
  [
    "logSession",
    [
      "Super+Alt+l"
    ]
  ],
  [
    "exportText",
    [
      "Super+s"
    ]
  ],
  [
    "pasteOneLine",
    [
      "Super+Alt+v"
    ]
  ],
  [
    "pasteEscaped",
    [
      "Super+Shift+v"
    ]
  ],
  [
    "saveArrangement",
    [
      "Super+Shift+s"
    ]
  ],
  [
    "restoreArrangement",
    [
      "Super+Shift+r"
    ]
  ]
]"##,
        )
        .expect("terminal keys bindings")
    })
}
pub fn symbols() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"{
  "Super": "❖",
  "Ctrl": "⌃",
  "Alt": "⌥",
  "Shift": "⇧",
  "ArrowUp": "↑",
  "ArrowDown": "↓",
  "ArrowLeft": "←",
  "ArrowRight": "→",
  "Enter": "⏎",
  "PageUp": "RePág",
  "PageDown": "AvPág",
  "Home": "Inicio",
  "End": "Fin",
  "Insert": "Insert",
  "Tab": "Tab",
  "Backspace": "⌫",
  "Delete": "Supr",
  "Escape": "Esc",
  "Space": "Espacio"
}"##,
        )
        .expect("terminal keys symbols")
    })
}
