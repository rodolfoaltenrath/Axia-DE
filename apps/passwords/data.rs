//! Built-in vault templates and colors. These contain no entry values.
use serde_json::Value;
use std::sync::OnceLock;
pub fn builtin() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"[
  {
    "id": "general",
    "icon": "folder",
    "color": "#8e8e93",
    "fields": [
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "url",
        "t": "url"
      }
    ]
  },
  {
    "id": "internet",
    "icon": "globe",
    "color": "#0a84ff",
    "fields": [
      {
        "k": "url",
        "t": "url"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "totp",
        "t": "totp"
      }
    ]
  },
  {
    "id": "email",
    "icon": "mail",
    "color": "#5e5ce6",
    "fields": [
      {
        "k": "email",
        "t": "email"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "imap_host",
        "t": "text"
      },
      {
        "k": "imap_port",
        "t": "number"
      },
      {
        "k": "smtp_host",
        "t": "text"
      },
      {
        "k": "smtp_port",
        "t": "number"
      },
      {
        "k": "app_password",
        "t": "hidden"
      }
    ]
  },
  {
    "id": "finance",
    "icon": "bank",
    "color": "#30d158",
    "fields": [
      {
        "k": "bank",
        "t": "text"
      },
      {
        "k": "client",
        "t": "text"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "transfer_key",
        "t": "hidden"
      },
      {
        "k": "pin",
        "t": "pin"
      },
      {
        "k": "coords",
        "t": "hidden",
        "multiline": true
      }
    ]
  },
  {
    "id": "cards",
    "icon": "card",
    "color": "#ff9f0a",
    "fields": [
      {
        "k": "holder",
        "t": "text"
      },
      {
        "k": "number",
        "t": "hidden"
      },
      {
        "k": "expiry",
        "t": "text"
      },
      {
        "k": "cvv",
        "t": "pin"
      },
      {
        "k": "pin",
        "t": "pin"
      },
      {
        "k": "phone",
        "t": "text"
      }
    ]
  },
  {
    "id": "network",
    "icon": "network",
    "color": "#64d2ff",
    "fields": [
      {
        "k": "device",
        "t": "text"
      },
      {
        "k": "host",
        "t": "text"
      },
      {
        "k": "port",
        "t": "number"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ]
  },
  {
    "id": "wifi",
    "icon": "wifi",
    "color": "#32ade6",
    "fields": [
      {
        "k": "ssid",
        "t": "text"
      },
      {
        "k": "security",
        "t": "text",
        "choices": [
          "WPA3",
          "WPA2",
          "WPA",
          "WEP",
          "none"
        ]
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "hidden",
        "t": "bool"
      }
    ]
  },
  {
    "id": "ssh",
    "icon": "terminal",
    "color": "#636366",
    "fields": [
      {
        "k": "host",
        "t": "text"
      },
      {
        "k": "port",
        "t": "number"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "private_key",
        "t": "hidden",
        "multiline": true
      },
      {
        "k": "fingerprint",
        "t": "text"
      }
    ]
  },
  {
    "id": "database",
    "icon": "database",
    "color": "#bf5af2",
    "fields": [
      {
        "k": "engine",
        "t": "text",
        "choices": [
          "PostgreSQL",
          "MySQL",
          "MariaDB",
          "SQLite",
          "MongoDB",
          "Redis",
          "SQL Server",
          "Oracle"
        ]
      },
      {
        "k": "host",
        "t": "text"
      },
      {
        "k": "port",
        "t": "number"
      },
      {
        "k": "database",
        "t": "text"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ]
  },
  {
    "id": "apps",
    "icon": "apps",
    "color": "#ff375f",
    "fields": [
      {
        "k": "app",
        "t": "text"
      },
      {
        "k": "account",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      },
      {
        "k": "totp",
        "t": "totp"
      },
      {
        "k": "recovery_codes",
        "t": "hidden",
        "multiline": true
      }
    ]
  },
  {
    "id": "licenses",
    "icon": "key",
    "color": "#ffd60a",
    "fields": [
      {
        "k": "product",
        "t": "text"
      },
      {
        "k": "version",
        "t": "text"
      },
      {
        "k": "license_key",
        "t": "hidden",
        "multiline": true
      },
      {
        "k": "email",
        "t": "email"
      },
      {
        "k": "purchase_date",
        "t": "date"
      },
      {
        "k": "expiry",
        "t": "date"
      }
    ],
    "attachments": true
  },
  {
    "id": "api",
    "icon": "braces",
    "color": "#ac8e68",
    "fields": [
      {
        "k": "service",
        "t": "text"
      },
      {
        "k": "key",
        "t": "hidden"
      },
      {
        "k": "secret",
        "t": "hidden"
      },
      {
        "k": "expiry",
        "t": "date"
      },
      {
        "k": "scopes",
        "t": "text"
      }
    ]
  },
  {
    "id": "identity",
    "icon": "id",
    "color": "#ff6961",
    "fields": [
      {
        "k": "doc_type",
        "t": "text",
        "choices": [
          "dni",
          "passport",
          "license",
          "health"
        ]
      },
      {
        "k": "number",
        "t": "text"
      },
      {
        "k": "holder",
        "t": "text"
      },
      {
        "k": "issued",
        "t": "date"
      },
      {
        "k": "expiry",
        "t": "date"
      },
      {
        "k": "issuer",
        "t": "text"
      }
    ],
    "attachments": true,
    "photos": true
  },
  {
    "id": "crypto",
    "icon": "coin",
    "color": "#f5a623",
    "fields": [
      {
        "k": "wallet",
        "t": "text"
      },
      {
        "k": "address",
        "t": "text"
      },
      {
        "k": "seed",
        "t": "hidden",
        "multiline": true
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ],
    "warning": true
  },
  {
    "id": "vpn",
    "icon": "shield",
    "color": "#34c759",
    "fields": [
      {
        "k": "server",
        "t": "text"
      },
      {
        "k": "vpn_type",
        "t": "text",
        "choices": [
          "WireGuard",
          "OpenVPN",
          "IPsec/IKEv2",
          "AnyConnect"
        ]
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ],
    "attachments": true,
    "settingsLink": true
  },
  {
    "id": "notes",
    "icon": "note",
    "color": "#ffcc00",
    "fields": [],
    "attachments": true,
    "notesFirst": true
  }
]"##,
        )
        .expect("bundled vault builtin")
    })
}
pub fn system() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"[
  {
    "id": "keyring",
    "icon": "lock",
    "color": "#8e8e93",
    "fields": [
      {
        "k": "service",
        "t": "text"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ]
  },
  {
    "id": "secrets",
    "icon": "key",
    "color": "#8e8e93",
    "fields": [
      {
        "k": "url",
        "t": "url"
      },
      {
        "k": "username",
        "t": "text"
      },
      {
        "k": "password",
        "t": "hidden"
      }
    ]
  }
]"##,
        )
        .expect("bundled vault system")
    })
}
pub fn types() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"[
  "text",
  "hidden",
  "url",
  "date",
  "number",
  "email",
  "totp",
  "file",
  "pin",
  "multiline"
]"##,
        )
        .expect("bundled vault types")
    })
}
pub fn colors() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"[
  "#0a84ff",
  "#5e5ce6",
  "#bf5af2",
  "#ff375f",
  "#ff453a",
  "#ff9f0a",
  "#ffd60a",
  "#30d158",
  "#64d2ff",
  "#ac8e68",
  "#8e8e93"
]"##,
        )
        .expect("bundled vault colors")
    })
}
