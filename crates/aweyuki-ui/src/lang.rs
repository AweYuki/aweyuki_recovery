use anyhow::{Context, Result};
use icu_locale_core::Locale;

mod vi;

/// A language the pages speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    /// Chinese in simplified characters, as mainland China writes it.
    Hans,
    /// Chinese in traditional characters, as Taiwan writes it.
    Hant,
    En,
    /// Vietnamese.
    Vi,
}

/// One text written in each language, in the order of [`Lang::ALL`].
pub type Text = [&'static str; 3];

impl Lang {
    /// The languages, in the order the language page lists them.
    pub const ALL: [Lang; 4] = [Lang::Hans, Lang::Hant, Lang::En, Lang::Vi];

    /// The language's name, written in it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Hans => "简体中文",
            Lang::Hant => "繁體中文",
            Lang::En => "English",
            Lang::Vi => "Tiếng Việt",
        }
    }

    /// The language as a locale, which [`Lang::for_locale`] reads back.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::Hans => "zh-Hans",
            Lang::Hant => "zh-Hant",
            Lang::En => "en",
            Lang::Vi => "vi",
        }
    }

    /// Of the same thing written in each language, in the order of
    /// [`Lang::ALL`], the one in this language. Vietnamese is kept apart
    /// from the texts the pages write: it is looked up by the Chinese and
    /// English texts, so two places that share an English word still tell
    /// their own meaning, and a page that has not been translated yet falls
    /// back to English.
    pub fn pick(self, [hans, hant, en]: [&'static str; 3]) -> &'static str {
        match self {
            Lang::Hans => hans,
            Lang::Hant => hant,
            Lang::En => en,
            Lang::Vi => vi::text(hans, en),
        }
    }

    /// The language for a locale: the `zh-CN` a system is built to start
    /// in, or the one it hands the recovery with the work it asks for,
    /// which older systems write `zh_CN`; Vietnamese; Chinese in that
    /// script or, with none named, in the one Taiwan, Hong Kong and Macau
    /// write in traditional characters, any other Chinese in simplified
    /// ones, and every other language in English.
    pub fn for_locale(tag: &str) -> Result<Lang> {
        // ICU takes a hyphen alone between the parts of a locale.
        let locale = Locale::try_from_str(&tag.replace('_', "-")).with_context(|| format!("read the locale {tag:?}"))?;
        let id = locale.id;
        if id.language.as_str() == "vi" {
            return Ok(Lang::Vi);
        }
        if id.language.as_str() != "zh" {
            return Ok(Lang::En);
        }
        let traditional = match id.script {
            Some(script) => script.as_str() == "Hant",
            None => id.region.is_some_and(|r| matches!(r.as_str(), "TW" | "HK" | "MO")),
        };
        Ok(if traditional { Lang::Hant } else { Lang::Hans })
    }
}

/// A text written in each language, made as [`format!`] makes it of the
/// values it names:
/// `tr!(l, "挂载 {base} 失败", "掛載 {base} 失敗", "Couldn't mount {base}", "Không thể gắn {base}")`.
#[macro_export]
macro_rules! tr {
    ($l:expr, $hans:literal, $hant:literal, $en:literal, $vi:literal $(,)?) => {
        match $l {
            $crate::lang::Lang::Hans => format!($hans),
            $crate::lang::Lang::Hant => format!($hant),
            $crate::lang::Lang::En => format!($en),
            $crate::lang::Lang::Vi => format!($vi),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::{TokenStream, TokenTree};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use syn::visit::Visit;

    /// The texts a source file writes: its string literals, those among a
    /// macro's arguments too.
    #[derive(Default)]
    struct Texts(Vec<String>);

    impl Texts {
        fn tokens(&mut self, tokens: TokenStream) {
            for tree in tokens {
                match tree {
                    TokenTree::Literal(lit) => {
                        if let syn::Lit::Str(s) = syn::Lit::new(lit) {
                            self.0.push(s.value());
                        }
                    }
                    TokenTree::Group(group) => self.tokens(group.stream()),
                    TokenTree::Ident(_) | TokenTree::Punct(_) => {}
                }
            }
        }
    }

    impl<'ast> Visit<'ast> for Texts {
        fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
            self.0.push(lit.value());
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.tokens(mac.tokens.clone());
        }
    }

    /// Read into `texts` what every source file in `folder` writes, those
    /// in the folders under it too.
    fn read(texts: &mut Texts, folder: &Path) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read(texts, &path);
                continue;
            }
            let file = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            texts.visit_file(&file);
        }
    }

    #[test]
    fn the_typeface_draws_every_text_the_pages_write() {
        let f = crate::font::bundled();
        let mut texts = Texts::default();
        read(&mut texts, &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        assert!(texts.0.iter().any(|t| t.contains("簽章")), "the texts in traditional characters are read");
        let missing: BTreeSet<char> = texts.0.iter().flat_map(|t| t.chars()).filter(|&ch| !ch.is_control() && !f.body.has(ch)).collect();
        assert!(missing.is_empty(), "MiSans draws none of {missing:?}");
    }

    fn is_test_only(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("test")
                || (a.path().is_ident("cfg") && matches!(&a.meta, syn::Meta::List(l) if l.tokens.to_string().trim() == "test"))
        })
    }

    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| ('\u{3000}'..='\u{9fff}').contains(&c))
    }

    #[derive(Default)]
    struct Triples(Vec<(String, String)>);

    impl<'ast> Visit<'ast> for Triples {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            if !is_test_only(&node.attrs) {
                syn::visit::visit_item_mod(self, node);
            }
        }

        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            if !is_test_only(&node.attrs) {
                syn::visit::visit_item_fn(self, node);
            }
        }

        fn visit_expr_array(&mut self, node: &'ast syn::ExprArray) {
            if let [a, b, c] = node.elems.iter().collect::<Vec<_>>().as_slice()
                && let (
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(hans), .. }),
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(hant), .. }),
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(en), .. }),
                ) = (a, b, c)
            {
                let (hans, hant, en) = (hans.value(), hant.value(), en.value());
                if has_cjk(&hans) || has_cjk(&hant) || (hans == "WLAN" && en == "Wi-Fi") || (hans == "UTC" && en == "UTC") {
                    self.0.push((hans, en));
                }
            }
            syn::visit::visit_expr_array(self, node);
        }
    }

    fn read_triples(triples: &mut Triples, folder: &Path) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read_triples(triples, &path);
                continue;
            }
            if path.file_name().is_some_and(|n| n == "vi.rs") {
                continue;
            }
            let file = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            triples.visit_file(&file);
        }
    }

    #[test]
    fn every_text_picked_has_a_vietnamese_translation() {
        let mut triples = Triples::default();
        read_triples(&mut triples, &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        assert!(!triples.0.is_empty());
        let missing: BTreeSet<(String, String)> = triples.0.into_iter().filter(|(hans, en)| vi::lookup(hans, en).is_none()).collect();
        assert!(missing.is_empty(), "missing Vietnamese translations: {missing:#?}");
    }

    #[test]
    fn a_locale_picks_the_language_and_its_characters() {
        let of = |tag| Lang::for_locale(tag).unwrap();
        assert_eq!(of("zh-CN"), Lang::Hans, "a China system, as the 17 Ultra's");
        assert_eq!(of("zh_CN"), Lang::Hans, "as older systems write it");
        assert_eq!(of("zh-SG"), Lang::Hans);
        assert_eq!(of("zh"), Lang::Hans);
        assert_eq!(of("zh-TW"), Lang::Hant);
        assert_eq!(of("zh-HK"), Lang::Hant);
        assert_eq!(of("zh-Hant-TW"), Lang::Hant);
        assert_eq!(of("zh-Hans-HK"), Lang::Hans, "a script named wins over the region");
        assert_eq!(of("en-US"), Lang::En);
        assert_eq!(of("vi-VN"), Lang::Vi, "a Vietnamese system");
        assert_eq!(of("vi"), Lang::Vi);
        assert_eq!(of("ja-JP"), Lang::En, "a language the pages do not speak");
        assert!(Lang::for_locale("not a locale").is_err());
    }

    #[test]
    fn each_language_s_locale_reads_back_as_it() {
        for lang in Lang::ALL {
            assert_eq!(Lang::for_locale(lang.tag()).unwrap(), lang);
        }
    }

    #[test]
    fn a_text_is_picked_and_made_in_the_language() {
        assert_eq!(Lang::Hant.pick(["设置", "設定", "Settings"]), "設定");
        assert_eq!(Lang::Vi.pick(["已锁定", "已鎖定", "Locked"]), "Đã khóa");
        assert_eq!(Lang::Vi.pick(["未解密", "未解密", "Locked"]), "Chưa giải mã");
        assert_eq!(Lang::Vi.pick(["安装", "安裝", "Install"]), "Cài đặt");
        assert_eq!(Lang::Vi.pick(["重新安装", "重新安裝", "Install"]), "cài lại");
        let base = "vendor";
        assert_eq!(tr!(Lang::En, "挂载 {base} 失败", "掛載 {base} 失敗", "Couldn't mount {base}", "Không gắn kết được {base}"), "Couldn't mount vendor");
        assert_eq!(Lang::ALL.map(Lang::name), ["简体中文", "繁體中文", "English", "Tiếng Việt"]);
    }
}
