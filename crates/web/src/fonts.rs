//! Inter Variable 4 (SIL OFL 1.1, `assets/fonts/OFL.txt`), self-hosted as
//! bundled assets: the same files omg-website serves from
//! `@fontsource-variable/inter` 5.3.0. No font CDN is contacted.
//!
//! The latin-ext face omits Fontsource's U+1E00-1E9F and U+1EF2-1EFF ranges:
//! `U+1EF2` cannot be written in the `font!` macro (Rust lexes `1E..` as a
//! float exponent). Those characters fall back to the system font.

use topcoat::{
    asset::asset,
    font::{Font, font},
};

pub const INTER: Font = font! {
    "Inter Variable",
    @font-face {
        src: url(asset!("assets/fonts/inter-latin-wght-normal.woff2")) format("woff2") tech("variations");
        font-weight: 100 900;
        font-style: normal;
        font-display: swap;
        unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC,
            U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212,
            U+2215, U+FEFF, U+FFFD;
    }
    @font-face {
        src: url(asset!("assets/fonts/inter-latin-ext-wght-normal.woff2")) format("woff2") tech("variations");
        font-weight: 100 900;
        font-style: normal;
        font-display: swap;
        unicode-range: U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304,
            U+0308, U+0329, U+1D00-1DBF, U+2020, U+20A0-20AB,
            U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF;
    }
};
