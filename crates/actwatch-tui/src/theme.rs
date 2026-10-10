//! Monochrome presentation mapping for [`Category`].
//!
//! Zero-hue palette: categories are distinguished by gray shade + fill
//! pattern so the dashboard stays readable without color.

use actwatch_core::Category;
use ratatui::style::Color;

/// Legend label, e.g. "Productivity & Finance".
pub fn name(cat: Category) -> &'static str {
    match cat {
        Category::Social => "Social",
        Category::ProductivityFinance => "Productivity & Finance",
        Category::Utilities => "Utilities",
        Category::Gaming => "Gaming",
        Category::Media => "Media",
        Category::Other => "Other",
    }
}

/// Compact label for table cells.
pub fn short_name(cat: Category) -> &'static str {
    match cat {
        Category::Social => "Social",
        Category::ProductivityFinance => "Productivity",
        Category::Utilities => "Utilities",
        Category::Gaming => "Gaming",
        Category::Media => "Media",
        Category::Other => "Other",
    }
}

/// Distinct fill-pattern character per category (stacked bars, gauges).
pub fn fill_char(cat: Category) -> char {
    match cat {
        Category::Social => '█',
        Category::ProductivityFinance => '▓',
        Category::Utilities => '▒',
        Category::Gaming => '░',
        Category::Media => '▚',
        Category::Other => '▤',
    }
}

/// Gray shade per category: bg #0B0B0C, text #F4F4F5, muted #8E8E93.
pub fn gray_shade(cat: Category) -> Color {
    match cat {
        Category::Social => Color::Rgb(212, 212, 216),
        Category::ProductivityFinance => Color::Rgb(161, 161, 170),
        Category::Utilities => Color::Rgb(113, 113, 122),
        Category::Gaming => Color::Rgb(228, 228, 231),
        Category::Media => Color::Rgb(82, 82, 91),
        Category::Other => Color::Rgb(63, 63, 70),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_distinct_fill_and_shade() {
        let cats = Category::all();
        let mut fills = std::collections::HashSet::new();
        let mut shades = std::collections::HashSet::new();
        for cat in cats {
            assert!(fills.insert(fill_char(cat)), "dup fill for {cat:?}");
            let Color::Rgb(r, g, b) = gray_shade(cat) else {
                panic!("non-rgb shade for {cat:?}");
            };
            assert!(shades.insert((r, g, b)), "dup shade for {cat:?}");
            assert!(!name(cat).is_empty());
            assert!(!short_name(cat).is_empty());
        }
    }
}
