//! User-supplied terminal themes loaded from `~/.config/roux/themes/`.
//!
//! Format: iTerm2 `.itermcolors` (XML plist). Each file becomes one theme;
//! the theme `id` is `"user:" + filename-stem` and the `label` is the stem
//! title-cased. Files that fail to parse are skipped — a single bad file
//! does not break the whole list. Callers (the Tauri command) are
//! expected to surface parse failures via the logger; this module only
//! returns the successes.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 16-color ANSI palette plus the special UI slots, mirroring the frontend
/// `TerminalTheme` shape so the bindings stay 1:1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAnsiPalette {
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalThemePalette {
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection_background: String,
    pub ansi: TerminalAnsiPalette,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UserTerminalTheme {
    /// `"user:" + filename-stem`. Stable across reloads, so settings can
    /// persist a reference to a user theme even when the file is briefly
    /// missing.
    pub id: String,
    /// Human label derived from the filename stem.
    pub label: String,
    pub palette: TerminalThemePalette,
}

#[derive(Debug, thiserror::Error)]
pub enum UserThemeError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("plist parse error in {path}: {source}")]
    Plist {
        path: String,
        #[source]
        source: plist::Error,
    },
    #[error("missing required key '{key}' in {path}")]
    MissingKey { path: String, key: &'static str },
    #[error("invalid color value for '{key}' in {path}")]
    InvalidColor { path: String, key: &'static str },
    #[error("filename has no usable stem: {path}")]
    InvalidFilename { path: String },
}

/// Scan the directory for `.itermcolors` files and return the parsed
/// themes. Sorted by `id` for stable UI ordering. Files that fail to
/// parse are returned as `Err` entries so the caller can log them while
/// still surfacing the good ones.
pub fn scan_user_terminal_themes(
    dir: &Path,
) -> Vec<Result<UserTerminalTheme, UserThemeError>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        // Missing directory is normal (user hasn't created it yet); return
        // an empty list rather than an error.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            return vec![Err(UserThemeError::Io {
                path: dir.display().to_string(),
                source: e,
            })];
        }
    };

    let mut results: Vec<Result<UserTerminalTheme, UserThemeError>> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_iterm = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("itermcolors"))
            .unwrap_or(false);
        if !is_iterm {
            continue;
        }
        results.push(parse_itermcolors_file(&path));
    }
    results.sort_by(|a, b| match (a, b) {
        (Ok(x), Ok(y)) => x.id.cmp(&y.id),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => std::cmp::Ordering::Equal,
    });
    results
}

fn parse_itermcolors_file(path: &Path) -> Result<UserTerminalTheme, UserThemeError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| UserThemeError::InvalidFilename {
            path: path.display().to_string(),
        })?;
    let id = format!("user:{stem}");
    let label = stem_to_label(stem);

    let value = plist::Value::from_file(path).map_err(|e| UserThemeError::Plist {
        path: path.display().to_string(),
        source: e,
    })?;
    let palette = palette_from_plist(&value, path)?;

    Ok(UserTerminalTheme {
        id,
        label,
        palette,
    })
}

fn palette_from_plist(value: &plist::Value, path: &Path) -> Result<TerminalThemePalette, UserThemeError> {
    let dict = value.as_dictionary().ok_or_else(|| UserThemeError::MissingKey {
        path: path.display().to_string(),
        key: "<root dict>",
    })?;
    let read = |key: &'static str| -> Result<String, UserThemeError> {
        let entry = dict.get(key).ok_or_else(|| UserThemeError::MissingKey {
            path: path.display().to_string(),
            key,
        })?;
        let dict = entry
            .as_dictionary()
            .ok_or_else(|| UserThemeError::InvalidColor {
                path: path.display().to_string(),
                key,
            })?;
        let comp = |k: &str| -> Result<f64, UserThemeError> {
            // iTerm2 usually writes <real>0.5</real> but occasionally
            // emits <integer>0</integer> / <integer>1</integer> for exact
            // endpoints, so accept both.
            dict.get(k)
                .and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64)))
                .ok_or_else(|| UserThemeError::InvalidColor {
                    path: path.display().to_string(),
                    key,
                })
        };
        let r = comp("Red Component")?;
        let g = comp("Green Component")?;
        let b = comp("Blue Component")?;
        Ok(rgb_to_hex(r, g, b))
    };

    Ok(TerminalThemePalette {
        background: read("Background Color")?,
        foreground: read("Foreground Color")?,
        cursor: read("Cursor Color")?,
        // iTerm uses "Selection Color" for the background of selected text;
        // there's no separate selection-foreground in the spec.
        selection_background: read("Selection Color")?,
        ansi: TerminalAnsiPalette {
            black: read("Ansi 0 Color")?,
            red: read("Ansi 1 Color")?,
            green: read("Ansi 2 Color")?,
            yellow: read("Ansi 3 Color")?,
            blue: read("Ansi 4 Color")?,
            magenta: read("Ansi 5 Color")?,
            cyan: read("Ansi 6 Color")?,
            white: read("Ansi 7 Color")?,
            bright_black: read("Ansi 8 Color")?,
            bright_red: read("Ansi 9 Color")?,
            bright_green: read("Ansi 10 Color")?,
            bright_yellow: read("Ansi 11 Color")?,
            bright_blue: read("Ansi 12 Color")?,
            bright_magenta: read("Ansi 13 Color")?,
            bright_cyan: read("Ansi 14 Color")?,
            bright_white: read("Ansi 15 Color")?,
        },
    })
}

fn rgb_to_hex(r: f64, g: f64, b: f64) -> String {
    let to_byte = |c: f64| -> u8 {
        let clamped = if c.is_nan() {
            0.0
        } else {
            c.clamp(0.0, 1.0)
        };
        (clamped * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", to_byte(r), to_byte(g), to_byte(b))
}

fn stem_to_label(stem: &str) -> String {
    let parts = stem.split(|c: char| c == '-' || c == '_' || c == ' ');
    let mut out = String::new();
    for (i, part) in parts.filter(|p| !p.is_empty()).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            for c in first.to_uppercase() {
                out.push(c);
            }
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        stem.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_dracula(dir: &Path, name: &str) -> std::path::PathBuf {
        // Trimmed Dracula iTerm2 colors — only the keys the parser reads.
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Background Color</key>
  <dict><key>Red Component</key><real>0.156862</real><key>Green Component</key><real>0.164705</real><key>Blue Component</key><real>0.211764</real></dict>
  <key>Foreground Color</key>
  <dict><key>Red Component</key><real>0.972549</real><key>Green Component</key><real>0.972549</real><key>Blue Component</key><real>0.949019</real></dict>
  <key>Cursor Color</key>
  <dict><key>Red Component</key><real>0.972549</real><key>Green Component</key><real>0.972549</real><key>Blue Component</key><real>0.941176</real></dict>
  <key>Selection Color</key>
  <dict><key>Red Component</key><real>0.266666</real><key>Green Component</key><real>0.278431</real><key>Blue Component</key><real>0.352941</real></dict>
  <key>Ansi 0 Color</key>
  <dict><key>Red Component</key><real>0.0</real><key>Green Component</key><real>0.0</real><key>Blue Component</key><real>0.0</real></dict>
  <key>Ansi 1 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>0.333333</real><key>Blue Component</key><real>0.333333</real></dict>
  <key>Ansi 2 Color</key>
  <dict><key>Red Component</key><real>0.313725</real><key>Green Component</key><real>0.980392</real><key>Blue Component</key><real>0.482352</real></dict>
  <key>Ansi 3 Color</key>
  <dict><key>Red Component</key><real>0.945098</real><key>Green Component</key><real>0.980392</real><key>Blue Component</key><real>0.549019</real></dict>
  <key>Ansi 4 Color</key>
  <dict><key>Red Component</key><real>0.741176</real><key>Green Component</key><real>0.576470</real><key>Blue Component</key><real>0.976470</real></dict>
  <key>Ansi 5 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>0.474509</real><key>Blue Component</key><real>0.776470</real></dict>
  <key>Ansi 6 Color</key>
  <dict><key>Red Component</key><real>0.545098</real><key>Green Component</key><real>0.913725</real><key>Blue Component</key><real>0.992156</real></dict>
  <key>Ansi 7 Color</key>
  <dict><key>Red Component</key><real>0.972549</real><key>Green Component</key><real>0.972549</real><key>Blue Component</key><real>0.949019</real></dict>
  <key>Ansi 8 Color</key>
  <dict><key>Red Component</key><real>0.384313</real><key>Green Component</key><real>0.447058</real><key>Blue Component</key><real>0.643137</real></dict>
  <key>Ansi 9 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>0.431372</real><key>Blue Component</key><real>0.431372</real></dict>
  <key>Ansi 10 Color</key>
  <dict><key>Red Component</key><real>0.411764</real><key>Green Component</key><real>1.0</real><key>Blue Component</key><real>0.580392</real></dict>
  <key>Ansi 11 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>1.0</real><key>Blue Component</key><real>0.647058</real></dict>
  <key>Ansi 12 Color</key>
  <dict><key>Red Component</key><real>0.839215</real><key>Green Component</key><real>0.674509</real><key>Blue Component</key><real>1.0</real></dict>
  <key>Ansi 13 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>0.572549</real><key>Blue Component</key><real>0.874509</real></dict>
  <key>Ansi 14 Color</key>
  <dict><key>Red Component</key><real>0.643137</real><key>Green Component</key><real>1.0</real><key>Blue Component</key><real>1.0</real></dict>
  <key>Ansi 15 Color</key>
  <dict><key>Red Component</key><real>1.0</real><key>Green Component</key><real>1.0</real><key>Blue Component</key><real>1.0</real></dict>
</dict>
</plist>"#;
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn rgb_round_trip() {
        assert_eq!(rgb_to_hex(0.0, 0.0, 0.0), "#000000");
        assert_eq!(rgb_to_hex(1.0, 1.0, 1.0), "#ffffff");
        assert_eq!(rgb_to_hex(1.0, 0.333333, 0.333333), "#ff5555");
    }

    #[test]
    fn rgb_clamps_out_of_range() {
        assert_eq!(rgb_to_hex(-0.5, 1.5, 0.5), "#00ff80");
    }

    #[test]
    fn stem_to_label_title_cases_kebab_and_snake() {
        assert_eq!(stem_to_label("dracula"), "Dracula");
        assert_eq!(stem_to_label("dracula-mod"), "Dracula Mod");
        assert_eq!(stem_to_label("tokyo_night_storm"), "Tokyo Night Storm");
        assert_eq!(stem_to_label("Solarized Light"), "Solarized Light");
    }

    #[test]
    fn parses_itermcolors_fixture() {
        let dir = tempfile::tempdir().unwrap();
        write_dracula(dir.path(), "dracula.itermcolors");
        let results = scan_user_terminal_themes(dir.path());
        assert_eq!(results.len(), 1);
        let theme = results[0].as_ref().unwrap();
        assert_eq!(theme.id, "user:dracula");
        assert_eq!(theme.label, "Dracula");
        assert_eq!(theme.palette.background, "#282a36");
        assert_eq!(theme.palette.ansi.red, "#ff5555");
        assert_eq!(theme.palette.ansi.bright_white, "#ffffff");
    }

    #[test]
    fn missing_directory_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let results = scan_user_terminal_themes(&missing);
        assert!(results.is_empty());
    }

    #[test]
    fn skips_non_itermcolors_files() {
        let dir = tempfile::tempdir().unwrap();
        write_dracula(dir.path(), "dracula.itermcolors");
        fs::write(dir.path().join("README.md"), "hi").unwrap();
        fs::write(dir.path().join("ignored.txt"), "x").unwrap();
        let results = scan_user_terminal_themes(dir.path());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn accepts_integer_components_for_exact_endpoints() {
        // iTerm2 sometimes writes <integer>0</integer> / <integer>1</integer>
        // when the value is exactly 0 or 1.
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Background Color</key>
  <dict><key>Red Component</key><integer>0</integer><key>Green Component</key><integer>0</integer><key>Blue Component</key><integer>0</integer></dict>
  <key>Foreground Color</key>
  <dict><key>Red Component</key><integer>1</integer><key>Green Component</key><integer>1</integer><key>Blue Component</key><integer>1</integer></dict>
  <key>Cursor Color</key>
  <dict><key>Red Component</key><real>0.5</real><key>Green Component</key><real>0.5</real><key>Blue Component</key><real>0.5</real></dict>
  <key>Selection Color</key>
  <dict><key>Red Component</key><real>0.2</real><key>Green Component</key><real>0.2</real><key>Blue Component</key><real>0.2</real></dict>"#;
        let mut full = body.to_string();
        for i in 0..16 {
            full.push_str(&format!(
                "\n  <key>Ansi {i} Color</key>\n  <dict><key>Red Component</key><integer>0</integer><key>Green Component</key><integer>0</integer><key>Blue Component</key><integer>0</integer></dict>"
            ));
        }
        full.push_str("\n</dict>\n</plist>\n");
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ints.itermcolors"), full).unwrap();
        let results = scan_user_terminal_themes(dir.path());
        assert_eq!(results.len(), 1);
        let theme = results[0].as_ref().unwrap();
        assert_eq!(theme.palette.background, "#000000");
        assert_eq!(theme.palette.foreground, "#ffffff");
    }

    #[test]
    fn malformed_file_surfaces_as_err_alongside_good_ones() {
        let dir = tempfile::tempdir().unwrap();
        write_dracula(dir.path(), "good.itermcolors");
        fs::write(dir.path().join("broken.itermcolors"), "not a plist").unwrap();
        let results = scan_user_terminal_themes(dir.path());
        assert_eq!(results.len(), 2);
        let oks = results.iter().filter(|r| r.is_ok()).count();
        let errs = results.iter().filter(|r| r.is_err()).count();
        assert_eq!(oks, 1);
        assert_eq!(errs, 1);
    }
}
