//! Theme: Material 3 palettes for four seed colors x light/dark, exactly
//! as the approved design (`new_design/`) defines them, plus a neutral
//! `mono` seed (white / near-black, not in the design). QML binds the role
//! props (`Theme.primary`, `Theme.surfaceContainerHigh`, ...) and never does
//! color math; the seed and dark choice live here.
//!
//! Generated from the design's `THEMES` table; edit the table, not the
//! props, when the palette changes.

use qtbridge::qobject;

/// Seeds in picker order: (id, swatch color).
pub const SEEDS: [(&str, &str); 5] = [
    ("mono", "#5e5e5e"),
    ("ember", "#e8710a"),
    ("ocean", "#2f6fe4"),
    ("forest", "#2e8b57"),
    ("orchid", "#9c4dcc"),
];

// One palette row, in field order: primary .. inversePrimary (see `resolve`).
type Row = [&'static str; 21];

const MONO_LIGHT: Row = [
    "#1b1b1b", "#ffffff", "#3a3a3a", "#ffffff", "#e3e3e3", "#1b1b1b", "#5e5e5e", "#c6c6c6", "#1b1b1b", "#ffffff", "#1b1b1b", "#474747", "#f7f7f7", "#f1f1f1", "#ebebeb", "#e2e2e2", "#777777", "#c6c6c6", "#303030", "#f1f1f1", "#c6c6c6",
];
const MONO_DARK: Row = [
    "#e2e2e2", "#1b1b1b", "#2a2a2a", "#e2e2e2", "#3a3a3a", "#e2e2e2", "#c6c6c6", "#5e5e5e", "#ffffff", "#000000", "#e2e2e2", "#c6c6c6", "#0e0e0e", "#161616", "#202020", "#2a2a2a", "#919191", "#474747", "#e2e2e2", "#303030", "#5e5e5e",
];
const EMBER_LIGHT: Row = [
    "#994700", "#ffffff", "#e8710a", "#4a1f00", "#feb386", "#78431f", "#0061a3", "#0097f8", "#002d4f", "#fff8f5", "#241913", "#574236", "#fff1ea", "#ffeadf", "#fae4d9", "#f4ded3", "#8b7264", "#dec1b0", "#3b2e26", "#ffede5", "#ffb68a",
];
const EMBER_DARK: Row = [
    "#ffb68a", "#522300", "#e8710a", "#4a1f00", "#6f3c18", "#f1a87b", "#9ecaff", "#0097f8", "#002d4f", "#1b110b", "#f4ded3", "#dec1b0", "#241913", "#291d16", "#342720", "#3f322b", "#a68b7d", "#574236", "#f4ded3", "#3b2e26", "#994700",
];
const OCEAN_LIGHT: Row = [
    "#0055c6", "#ffffff", "#2f6fe4", "#fefbff", "#b8cbff", "#435582", "#974300", "#be5600", "#fffbff", "#faf8ff", "#191b23", "#424654", "#f2f3fd", "#ededf7", "#e7e7f2", "#e1e2ec", "#737785", "#c2c6d6", "#2e3038", "#eff0fa", "#b0c6ff",
];
const OCEAN_DARK: Row = [
    "#b0c6ff", "#002d6f", "#2f6fe4", "#fefbff", "#354874", "#a5b8ea", "#ffb68e", "#be5600", "#fffbff", "#11131a", "#e1e2ec", "#c2c6d6", "#191b23", "#1d1f27", "#272a31", "#32353c", "#8c909f", "#424654", "#e1e2ec", "#2e3038", "#0055c6",
];
const FOREST_LIGHT: Row = [
    "#006a3b", "#ffffff", "#268451", "#f6fff4", "#c6ecce", "#4b6c54", "#973f4c", "#b55764", "#fffbff", "#f6fbf3", "#181d19", "#3f4941", "#f0f5ee", "#ebefe8", "#e5e9e2", "#dfe4dd", "#6f7a70", "#becabe", "#2d322d", "#edf2eb", "#7ed99e",
];
const FOREST_DARK: Row = [
    "#7ed99e", "#00391d", "#47a26b", "#001206", "#2d4e38", "#9abea2", "#ffb2b9", "#d8727f", "#270007", "#101511", "#dfe4dd", "#becabe", "#181d19", "#1c211d", "#262b27", "#313631", "#889489", "#3f4941", "#dfe4dd", "#2d322d", "#006a3b",
];
const ORCHID_LIGHT: Row = [
    "#8131b1", "#ffffff", "#9c4dcc", "#fff5fd", "#ebc3ff", "#6e4d80", "#705d00", "#c5aa40", "#4d3f00", "#fff7fc", "#1f1a21", "#4d4351", "#fcf0fc", "#f6eaf6", "#f0e5f0", "#eadfea", "#7f7383", "#d0c2d3", "#352e37", "#f9edf9", "#e5b4ff",
];
const ORCHID_DARK: Row = [
    "#e5b4ff", "#4f0077", "#9c4dcc", "#fff5fd", "#5c3c6f", "#d1aae5", "#e2c558", "#c5aa40", "#4d3f00", "#171119", "#eadfea", "#d0c2d3", "#1f1a21", "#231e26", "#2e2830", "#39333b", "#998c9d", "#4d4351", "#eadfea", "#352e37", "#8131b1",
];

/// Palette row for `seed` (unknown seeds fall back to ember).
pub fn palette(seed: &str, dark: bool) -> &'static Row {
    match (seed, dark) {
        ("mono", false) => &MONO_LIGHT,
        ("mono", true) => &MONO_DARK,
        ("ember", false) => &EMBER_LIGHT,
        ("ember", true) => &EMBER_DARK,
        ("ocean", false) => &OCEAN_LIGHT,
        ("ocean", true) => &OCEAN_DARK,
        ("forest", false) => &FOREST_LIGHT,
        ("forest", true) => &FOREST_DARK,
        ("orchid", false) => &ORCHID_LIGHT,
        ("orchid", true) => &ORCHID_DARK,
        (_, false) => &EMBER_LIGHT,
        (_, true) => &EMBER_DARK,
    }
}

/// Saved choice: `~/.config/spinwave/theme` holding "<seed> <dark|light>".
#[cfg(not(test))]
fn theme_file() -> Option<std::path::PathBuf> {
    Some(crate::paths::config_home()?.join("spinwave").join("theme"))
}

/// Parse a saved choice; `None` for anything unexpected (defaults win).
fn parse_saved(text: &str) -> Option<(String, bool)> {
    let mut parts = text.split_whitespace();
    let seed = parts.next()?;
    let dark = match parts.next()? {
        "dark" => true,
        "light" => false,
        _ => return None,
    };
    SEEDS.iter().any(|(id, _)| *id == seed).then(|| (seed.to_string(), dark))
}

/// QML `Theme` singleton: the seed + dark choice and the resolved roles.
pub struct Theme {
    seed: String,
    dark: bool,
    primary: String,
    on_primary: String,
    primary_container: String,
    on_primary_container: String,
    secondary_container: String,
    on_secondary_container: String,
    tertiary: String,
    tertiary_container: String,
    on_tertiary_container: String,
    surface: String,
    on_surface: String,
    on_surface_variant: String,
    surface_container_low: String,
    surface_container: String,
    surface_container_high: String,
    surface_container_highest: String,
    outline: String,
    outline_variant: String,
    inverse_surface: String,
    inverse_on_surface: String,
    inverse_primary: String,
}

impl Default for Theme {
    fn default() -> Self {
        let mut t = Self {
            seed: "ember".to_string(),
            dark: true,
            primary: String::new(),
            on_primary: String::new(),
            primary_container: String::new(),
            on_primary_container: String::new(),
            secondary_container: String::new(),
            on_secondary_container: String::new(),
            tertiary: String::new(),
            tertiary_container: String::new(),
            on_tertiary_container: String::new(),
            surface: String::new(),
            on_surface: String::new(),
            on_surface_variant: String::new(),
            surface_container_low: String::new(),
            surface_container: String::new(),
            surface_container_high: String::new(),
            surface_container_highest: String::new(),
            outline: String::new(),
            outline_variant: String::new(),
            inverse_surface: String::new(),
            inverse_on_surface: String::new(),
            inverse_primary: String::new(),
        };
        // Tests keep the fixed default instead of the user's saved choice.
        #[cfg(not(test))]
        if let Some((seed, dark)) = theme_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| parse_saved(&text))
        {
            t.seed = seed;
            t.dark = dark;
        }
        t.resolve();
        t
    }
}

impl Theme {
    /// Copy the active palette row into the role fields (signal-free).
    fn resolve(&mut self) {
        let row = palette(&self.seed, self.dark);
        self.primary = row[0].to_string();
        self.on_primary = row[1].to_string();
        self.primary_container = row[2].to_string();
        self.on_primary_container = row[3].to_string();
        self.secondary_container = row[4].to_string();
        self.on_secondary_container = row[5].to_string();
        self.tertiary = row[6].to_string();
        self.tertiary_container = row[7].to_string();
        self.on_tertiary_container = row[8].to_string();
        self.surface = row[9].to_string();
        self.on_surface = row[10].to_string();
        self.on_surface_variant = row[11].to_string();
        self.surface_container_low = row[12].to_string();
        self.surface_container = row[13].to_string();
        self.surface_container_high = row[14].to_string();
        self.surface_container_highest = row[15].to_string();
        self.outline = row[16].to_string();
        self.outline_variant = row[17].to_string();
        self.inverse_surface = row[18].to_string();
        self.inverse_on_surface = row[19].to_string();
        self.inverse_primary = row[20].to_string();
    }

    /// Best-effort save of the choice (a theme isn't worth an error).
    #[cfg(not(test))]
    fn save(&self) {
        if let Some(path) = theme_file() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mode = if self.dark { "dark" } else { "light" };
            let _ = std::fs::write(path, format!("{} {mode}\n", self.seed));
        }
    }

    fn refresh(&mut self) {
        self.resolve();
        #[cfg(not(test))]
        self.save();
        self.seed_changed();
        self.dark_changed();
        self.primary_changed();
        self.on_primary_changed();
        self.primary_container_changed();
        self.on_primary_container_changed();
        self.secondary_container_changed();
        self.on_secondary_container_changed();
        self.tertiary_changed();
        self.tertiary_container_changed();
        self.on_tertiary_container_changed();
        self.surface_changed();
        self.on_surface_changed();
        self.on_surface_variant_changed();
        self.surface_container_low_changed();
        self.surface_container_changed();
        self.surface_container_high_changed();
        self.surface_container_highest_changed();
        self.outline_changed();
        self.outline_variant_changed();
        self.inverse_surface_changed();
        self.inverse_on_surface_changed();
        self.inverse_primary_changed();
    }
}

#[qobject(Singleton, LinkMe, ConvertToCamelCase)]
impl Theme {
    qproperty!("seed", Member = seed, Notify = seed_changed);
    qproperty!("dark", Member = dark, Notify = dark_changed);
    qproperty!("primary", Member = primary, Notify = primary_changed);
    qproperty!("onPrimary", Member = on_primary, Notify = on_primary_changed);
    qproperty!("primaryContainer", Member = primary_container, Notify = primary_container_changed);
    qproperty!("onPrimaryContainer", Member = on_primary_container, Notify = on_primary_container_changed);
    qproperty!("secondaryContainer", Member = secondary_container, Notify = secondary_container_changed);
    qproperty!("onSecondaryContainer", Member = on_secondary_container, Notify = on_secondary_container_changed);
    qproperty!("tertiary", Member = tertiary, Notify = tertiary_changed);
    qproperty!("tertiaryContainer", Member = tertiary_container, Notify = tertiary_container_changed);
    qproperty!("onTertiaryContainer", Member = on_tertiary_container, Notify = on_tertiary_container_changed);
    qproperty!("surface", Member = surface, Notify = surface_changed);
    qproperty!("onSurface", Member = on_surface, Notify = on_surface_changed);
    qproperty!("onSurfaceVariant", Member = on_surface_variant, Notify = on_surface_variant_changed);
    qproperty!("surfaceContainerLow", Member = surface_container_low, Notify = surface_container_low_changed);
    qproperty!("surfaceContainer", Member = surface_container, Notify = surface_container_changed);
    qproperty!("surfaceContainerHigh", Member = surface_container_high, Notify = surface_container_high_changed);
    qproperty!("surfaceContainerHighest", Member = surface_container_highest, Notify = surface_container_highest_changed);
    qproperty!("outline", Member = outline, Notify = outline_changed);
    qproperty!("outlineVariant", Member = outline_variant, Notify = outline_variant_changed);
    qproperty!("inverseSurface", Member = inverse_surface, Notify = inverse_surface_changed);
    qproperty!("inverseOnSurface", Member = inverse_on_surface, Notify = inverse_on_surface_changed);
    qproperty!("inversePrimary", Member = inverse_primary, Notify = inverse_primary_changed);

    #[qsignal]
    fn seed_changed(&mut self);
    #[qsignal]
    fn dark_changed(&mut self);
    #[qsignal]
    fn primary_changed(&mut self);
    #[qsignal]
    fn on_primary_changed(&mut self);
    #[qsignal]
    fn primary_container_changed(&mut self);
    #[qsignal]
    fn on_primary_container_changed(&mut self);
    #[qsignal]
    fn secondary_container_changed(&mut self);
    #[qsignal]
    fn on_secondary_container_changed(&mut self);
    #[qsignal]
    fn tertiary_changed(&mut self);
    #[qsignal]
    fn tertiary_container_changed(&mut self);
    #[qsignal]
    fn on_tertiary_container_changed(&mut self);
    #[qsignal]
    fn surface_changed(&mut self);
    #[qsignal]
    fn on_surface_changed(&mut self);
    #[qsignal]
    fn on_surface_variant_changed(&mut self);
    #[qsignal]
    fn surface_container_low_changed(&mut self);
    #[qsignal]
    fn surface_container_changed(&mut self);
    #[qsignal]
    fn surface_container_high_changed(&mut self);
    #[qsignal]
    fn surface_container_highest_changed(&mut self);
    #[qsignal]
    fn outline_changed(&mut self);
    #[qsignal]
    fn outline_variant_changed(&mut self);
    #[qsignal]
    fn inverse_surface_changed(&mut self);
    #[qsignal]
    fn inverse_on_surface_changed(&mut self);
    #[qsignal]
    fn inverse_primary_changed(&mut self);

    /// Pick a seed (`mono | ember | ocean | forest | orchid`); unknown ids are ignored.
    #[qslot]
    fn set_seed(&mut self, seed: String) {
        if SEEDS.iter().any(|(id, _)| *id == seed) && seed != self.seed {
            self.seed = seed;
            self.refresh();
        }
    }

    #[qslot]
    fn set_dark(&mut self, dark: bool) {
        if dark != self.dark {
            self.dark = dark;
            self.refresh();
        }
    }

    /// Picker swatches as JSON `[{"id","hex"}]`.
    #[qslot]
    fn seeds_json(&self) -> String {
        let rows: Vec<serde_json::Value> = SEEDS
            .iter()
            .map(|(id, hex)| serde_json::json!({ "id": id, "hex": hex }))
            .collect();
        serde_json::to_string(&rows).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_row_is_hex() {
        for (seed, _) in SEEDS {
            for dark in [false, true] {
                for hex in palette(seed, dark) {
                    assert!(hex.len() == 7 && hex.starts_with('#'), "{seed} {dark}: {hex}");
                    assert!(hex[1..].chars().all(|c| c.is_ascii_hexdigit()));
                }
            }
        }
    }

    #[test]
    fn saved_choice_parses_strictly() {
        assert_eq!(parse_saved("mono light\n"), Some(("mono".to_string(), false)));
        assert_eq!(parse_saved("ocean dark"), Some(("ocean".to_string(), true)));
        assert_eq!(parse_saved("bogus dark"), None);
        assert_eq!(parse_saved("ocean"), None);
    }

    #[test]
    fn default_is_ember_dark_and_unknown_seed_falls_back() {
        let t = Theme::default();
        assert_eq!((t.seed.as_str(), t.dark), ("ember", true));
        assert_eq!(t.primary, "#ffb68a");
        assert_eq!(palette("bogus", false), palette("ember", false));
        assert_ne!(palette("ocean", true), palette("ocean", false));
    }
}
