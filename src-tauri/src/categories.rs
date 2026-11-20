use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Productivity {
    Productive,
    Neutral,
    Distracting,
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub name: &'static str,
    pub productivity: Productivity,
}

use Productivity::*;

/// Fixed category list. Apps are assigned to one of these, either by the
/// keyword guess below or by the user.
pub const CATEGORIES: &[Category] = &[
    Category {
        name: "Coding",
        productivity: Productive,
    },
    Category {
        name: "Design",
        productivity: Productive,
    },
    Category {
        name: "Writing",
        productivity: Productive,
    },
    Category {
        name: "Research",
        productivity: Productive,
    },
    Category {
        name: "Meetings",
        productivity: Productive,
    },
    Category {
        name: "Communication",
        productivity: Neutral,
    },
    Category {
        name: "Browsing",
        productivity: Neutral,
    },
    Category {
        name: "Utilities",
        productivity: Neutral,
    },
    Category {
        name: "Other",
        productivity: Neutral,
    },
    Category {
        name: "Entertainment",
        productivity: Distracting,
    },
    Category {
        name: "Social Media",
        productivity: Distracting,
    },
];

pub fn is_valid(name: &str) -> bool {
    CATEGORIES.iter().any(|c| c.name == name)
}

pub fn productivity_of(name: &str) -> Productivity {
    CATEGORIES
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.productivity)
        .unwrap_or(Neutral)
}

const RULES: &[(&str, &[&str])] = &[
    (
        "Coding",
        &[
            "code",
            "cursor",
            "xcode",
            "intellij",
            "pycharm",
            "webstorm",
            "goland",
            "clion",
            "rider",
            "rustrover",
            "android studio",
            "visual studio",
            "sublime",
            "vim",
            "emacs",
            "zed",
            "terminal",
            "iterm",
            "xterm",
            "konsole",
            "tilix",
            "ghostty",
            "hyper",
            "warp",
            "alacritty",
            "kitty",
            "wezterm",
            "powershell",
            "cmd",
            "windowsterminal",
            "docker",
            "postman",
            "insomnia",
            "github desktop",
            "sourcetree",
            "fork",
            "datagrip",
            "dbeaver",
            "tableplus",
            "jupyter",
        ],
    ),
    (
        "Design",
        &[
            "figma",
            "sketch",
            "photoshop",
            "illustrator",
            "affinity",
            "blender",
            "canva",
            "framer",
            "inkscape",
            "gimp",
            "premiere",
            "after effects",
            "final cut",
            "davinci",
        ],
    ),
    (
        "Writing",
        &[
            "word",
            "pages",
            "notion",
            "obsidian",
            "bear",
            "ulysses",
            "typora",
            "scrivener",
            "onenote",
            "evernote",
            "excel",
            "numbers",
            "powerpoint",
            "keynote",
            "libreoffice",
            "craft",
            "logseq",
        ],
    ),
    (
        "Research",
        &[
            "zotero", "mendeley", "acrobat", "preview", "skim", "kindle", "calibre",
        ],
    ),
    (
        "Meetings",
        &["zoom", "meet", "webex", "facetime", "gotomeeting"],
    ),
    (
        "Communication",
        &[
            "slack",
            "teams",
            "discord",
            "mail",
            "outlook",
            "thunderbird",
            "spark",
            "telegram",
            "whatsapp",
            "signal",
            "messages",
            "skype",
            "element",
        ],
    ),
    (
        "Browsing",
        &[
            "chrome", "firefox", "safari", "edge", "brave", "arc", "opera", "vivaldi", "chromium",
            "orion", "zen",
        ],
    ),
    (
        "Entertainment",
        &[
            "spotify",
            "music",
            "netflix",
            "vlc",
            "iina",
            "steam",
            "epic games",
            "battle.net",
            "tv",
            "podcasts",
            "plex",
        ],
    ),
    (
        "Social Media",
        &[
            "twitter",
            "tweetdeck",
            "instagram",
            "facebook",
            "tiktok",
            "reddit",
        ],
    ),
    (
        "Utilities",
        &[
            "finder",
            "explorer",
            "settings",
            "system preferences",
            "activity monitor",
            "task manager",
            "calculator",
            "1password",
            "bitwarden",
            "keychain",
            "calendar",
            "reminders",
            "todoist",
            "things",
            "stint",
        ],
    ),
];

/// Best-effort guess of a category from the app name.
pub fn guess(app: &str) -> &'static str {
    let lower = app.to_lowercase();
    // Whole-word match for short keywords (avoids "tv" matching "atvd"),
    // substring match for longer ones.
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '.')
        .filter(|w| !w.is_empty())
        .collect();
    for (cat, keywords) in RULES {
        for kw in *keywords {
            let hit = if kw.len() <= 4 && !kw.contains(' ') {
                words.iter().any(|w| w == kw)
            } else {
                lower.contains(kw)
            };
            if hit {
                return cat;
            }
        }
    }
    "Other"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses() {
        assert_eq!(guess("Code"), "Coding");
        assert_eq!(guess("Visual Studio Code"), "Coding");
        assert_eq!(guess("Google Chrome"), "Browsing");
        assert_eq!(guess("Slack"), "Communication");
        assert_eq!(guess("Spotify"), "Entertainment");
        assert_eq!(guess("Microsoft Word"), "Writing");
        assert_eq!(guess("Some Random Thing"), "Other");
        assert_eq!(guess("Mail"), "Communication");
        assert_eq!(guess("Gmailer"), "Other");
        assert_eq!(guess("XTerm"), "Coding");
    }

    #[test]
    fn all_rules_use_valid_categories() {
        for (cat, _) in RULES {
            assert!(is_valid(cat), "{cat}");
        }
    }
}
