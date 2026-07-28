use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, DeclarationParser, ParseError, ParseErrorKind,
    Parser, ParserInput, ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
    StyleSheetParser,
};

use super::{ThemeError, ThemeErrorCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleScope {
    Root,
    ChromePaint,
    Editor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PolicyAtRule {
    FontFace,
    NestedRules,
}

pub(super) fn validate_theme_css_policy(css: &str, theme_id: &str) -> Result<(), ThemeError> {
    let mut input = ParserInput::new(css);
    let mut input = Parser::new(&mut input);
    let mut parser = ThemePolicyParser { theme_id };
    for result in StyleSheetParser::new(&mut input, &mut parser) {
        if let Err((error, _)) = result {
            let code = error_code(error);
            let message = match code {
                ThemeErrorCode::UnsafeResource => {
                    "Theme CSS can only style the selected theme root and its Markdown editors."
                }
                _ => "Theme CSS contains invalid syntax.",
            };
            return Err(ThemeError::new(code, message));
        }
    }
    Ok(())
}

struct ThemePolicyParser<'theme> {
    theme_id: &'theme str,
}

impl<'i> QualifiedRuleParser<'i> for ThemePolicyParser<'_> {
    type Prelude = RuleScope;
    type QualifiedRule = ();
    type Error = ThemeErrorCode;

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        let start = input.position();
        consume_component_values(input)?;
        let selector = input.slice_from(start);
        validate_selector_list(selector, self.theme_id).map_err(|code| input.new_custom_error(code))
    }

    fn parse_block<'t>(
        &mut self,
        scope: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        validate_declarations(input, scope)
    }
}

impl<'i> AtRuleParser<'i> for ThemePolicyParser<'_> {
    type Prelude = PolicyAtRule;
    type AtRule = ();
    type Error = ThemeErrorCode;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        let rule = if name.eq_ignore_ascii_case("font-face") {
            PolicyAtRule::FontFace
        } else if matches_ignore_ascii_case(&name, &["media", "supports", "container", "layer"]) {
            PolicyAtRule::NestedRules
        } else {
            return Err(input.new_custom_error(ThemeErrorCode::UnsafeResource));
        };
        consume_component_values(input)?;
        Ok(rule)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        match prelude {
            PolicyAtRule::FontFace => validate_font_face(input),
            PolicyAtRule::NestedRules => {
                for result in StyleSheetParser::new(input, self) {
                    if let Err((error, _)) = result {
                        return Err(error);
                    }
                }
                Ok(())
            }
        }
    }
}

fn validate_declarations<'i, 't>(
    input: &mut Parser<'i, 't>,
    scope: RuleScope,
) -> Result<(), ParseError<'i, ThemeErrorCode>> {
    let mut parser = DeclarationPolicyParser { scope };
    for result in RuleBodyParser::new(input, &mut parser) {
        if let Err((error, _)) = result {
            return Err(error);
        }
    }
    Ok(())
}

fn validate_font_face<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<(), ParseError<'i, ThemeErrorCode>> {
    let mut parser = FontFacePolicyParser;
    for result in RuleBodyParser::new(input, &mut parser) {
        if let Err((error, _)) = result {
            return Err(error);
        }
    }
    Ok(())
}

struct DeclarationPolicyParser {
    scope: RuleScope,
}

impl<'i> DeclarationParser<'i> for DeclarationPolicyParser {
    type Declaration = ();
    type Error = ThemeErrorCode;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        let property = name.to_ascii_lowercase();
        let allowed = match self.scope {
            RuleScope::Root => root_property_allowed(&property),
            RuleScope::ChromePaint => chrome_paint_property_allowed(&property),
            RuleScope::Editor => editor_property_allowed(&property),
        };
        if !allowed {
            return Err(input.new_custom_error(ThemeErrorCode::UnsafeResource));
        }

        let start = input.position();
        consume_component_values(input)?;
        if input.slice_from(start).trim().is_empty() {
            return Err(input.new_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        if property == "position" {
            validate_safe_position(input.slice_from(start))
                .map_err(|code| input.new_custom_error(code))?;
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for DeclarationPolicyParser {
    type Prelude = ();
    type AtRule = ();
    type Error = ThemeErrorCode;
}

impl<'i> QualifiedRuleParser<'i> for DeclarationPolicyParser {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ThemeErrorCode;
}

impl<'i> RuleBodyItemParser<'i, (), ThemeErrorCode> for DeclarationPolicyParser {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

struct FontFacePolicyParser;

impl<'i> DeclarationParser<'i> for FontFacePolicyParser {
    type Declaration = ();
    type Error = ThemeErrorCode;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        if !matches_ignore_ascii_case(
            &name,
            &[
                "font-family",
                "src",
                "font-display",
                "font-style",
                "font-weight",
                "font-stretch",
                "unicode-range",
                "font-feature-settings",
                "font-variation-settings",
                "font-language-override",
                "font-named-instance",
                "ascent-override",
                "descent-override",
                "line-gap-override",
                "size-adjust",
            ],
        ) {
            return Err(input.new_custom_error(ThemeErrorCode::UnsafeResource));
        }
        let start = input.position();
        consume_component_values(input)?;
        if input.slice_from(start).trim().is_empty() {
            return Err(input.new_error(BasicParseErrorKind::QualifiedRuleInvalid));
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for FontFacePolicyParser {
    type Prelude = ();
    type AtRule = ();
    type Error = ThemeErrorCode;
}

impl<'i> QualifiedRuleParser<'i> for FontFacePolicyParser {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ThemeErrorCode;
}

impl<'i> RuleBodyItemParser<'i, (), ThemeErrorCode> for FontFacePolicyParser {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

fn consume_component_values<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<(), ParseError<'i, ThemeErrorCode>> {
    loop {
        let token = match input.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(error) if matches!(error.kind, BasicParseErrorKind::EndOfInput) => return Ok(()),
            Err(error) => return Err(ParseError::from(error)),
        };
        if token.is_parse_error() {
            return Err(input.new_error(BasicParseErrorKind::UnexpectedToken(token)));
        }
    }
}

fn validate_selector_list(
    selector_list: &str,
    theme_id: &str,
) -> Result<RuleScope, ThemeErrorCode> {
    if selector_list.contains(['\\', '\0'])
        || selector_list.contains("/*")
        || selector_list.contains("*/")
    {
        return Err(ThemeErrorCode::UnsafeResource);
    }

    let selectors = split_top_level_selectors(selector_list)?;
    let mut scope = None;
    for selector in selectors {
        let selector_scope = validate_selector(selector, theme_id)?;
        scope = Some(match (scope, selector_scope) {
            (None, next) => next,
            (Some(existing), next) if existing == next => existing,
            (
                Some(RuleScope::Editor | RuleScope::ChromePaint),
                RuleScope::Editor | RuleScope::ChromePaint,
            ) => RuleScope::ChromePaint,
            _ => return Err(ThemeErrorCode::UnsafeResource),
        });
    }
    scope.ok_or(ThemeErrorCode::InvalidCss)
}

fn validate_selector(selector: &str, theme_id: &str) -> Result<RuleScope, ThemeErrorCode> {
    let normalized = normalize_selector(selector);
    if normalized.is_empty() {
        return Err(ThemeErrorCode::InvalidCss);
    }
    let themed_root = format!(":root[data-theme=\"{theme_id}\"]");
    let themed_attribute = format!("[data-theme=\"{theme_id}\"]");
    if matches!(normalized.as_str(), ":root")
        || normalized == themed_root
        || normalized == themed_attribute
    {
        return Ok(RuleScope::Root);
    }

    let anchors = [
        format!(".markdown-paper[data-editor-theme=\"{theme_id}\"]"),
        format!(".markdown-source-paper[data-editor-theme=\"{theme_id}\"]"),
        format!("{themed_root} .markdown-paper"),
        format!("{themed_root} .markdown-source-paper"),
        format!("{themed_attribute} .markdown-paper"),
        format!("{themed_attribute} .markdown-source-paper"),
    ];
    let anchor = anchors
        .iter()
        .filter_map(|anchor| top_level_substring(&normalized, anchor).map(|index| (index, anchor)))
        .min_by_key(|(index, _)| *index);
    if let Some((anchor_index, anchor)) = anchor {
        let suffix = &normalized[anchor_index + anchor.len()..];
        if has_top_level_sibling_combinator(suffix) {
            return Err(ThemeErrorCode::UnsafeResource);
        }
        return Ok(RuleScope::Editor);
    }

    for root in [&themed_root, &themed_attribute] {
        if normalized.strip_prefix(root).is_some_and(|suffix| {
            suffix.starts_with(' ') && !has_top_level_sibling_combinator(suffix)
        }) {
            return Ok(RuleScope::ChromePaint);
        }
    }

    Err(ThemeErrorCode::UnsafeResource)
}

fn normalize_selector(selector: &str) -> String {
    let mut normalized = String::with_capacity(selector.len());
    let mut pending_space = false;
    let mut quote = None;
    for character in selector.trim().chars() {
        if let Some(active_quote) = quote {
            normalized.push(if character == '\'' { '"' } else { character });
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            normalized.push('"');
        } else if character.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space
                && !normalized.is_empty()
                && !matches!(character, ']' | ')' | ':' | '=' | '>' | '+' | '~')
                && !matches!(
                    normalized.chars().last(),
                    Some('[' | '(' | ':' | '=' | '>' | '+' | '~')
                )
            {
                normalized.push(' ');
            }
            pending_space = false;
            normalized.push(character);
        }
    }
    normalized
}

fn split_top_level_selectors(selector_list: &str) -> Result<Vec<&str>, ThemeErrorCode> {
    let mut selectors = Vec::new();
    let mut start = 0;
    let mut parentheses = 0_u32;
    let mut brackets = 0_u32;
    let mut quote = None;
    for (index, character) in selector_list.char_indices() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => parentheses = parentheses.saturating_add(1),
            ')' => {
                parentheses = parentheses
                    .checked_sub(1)
                    .ok_or(ThemeErrorCode::InvalidCss)?
            }
            '[' => brackets = brackets.saturating_add(1),
            ']' => brackets = brackets.checked_sub(1).ok_or(ThemeErrorCode::InvalidCss)?,
            ',' if parentheses == 0 && brackets == 0 => {
                selectors.push(selector_list[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || parentheses != 0 || brackets != 0 {
        return Err(ThemeErrorCode::InvalidCss);
    }
    selectors.push(selector_list[start..].trim());
    Ok(selectors)
}

fn top_level_substring(value: &str, needle: &str) -> Option<usize> {
    let mut parentheses = 0_u32;
    let mut quote = None;
    for (index, character) in value.char_indices() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => parentheses = parentheses.saturating_add(1),
            ')' => parentheses = parentheses.saturating_sub(1),
            _ if parentheses == 0 && value[index..].starts_with(needle) => return Some(index),
            _ => {}
        }
    }
    None
}

fn has_top_level_sibling_combinator(value: &str) -> bool {
    let mut parentheses = 0_u32;
    let mut brackets = 0_u32;
    let mut quote = None;
    for character in value.chars() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => parentheses = parentheses.saturating_add(1),
            ')' => parentheses = parentheses.saturating_sub(1),
            '[' => brackets = brackets.saturating_add(1),
            ']' => brackets = brackets.saturating_sub(1),
            '+' | '~' if parentheses == 0 && brackets == 0 => return true,
            _ => {}
        }
    }
    false
}

fn root_property_allowed(property: &str) -> bool {
    if property.starts_with("--") {
        return !property.starts_with("--compact-")
            && !property.starts_with("--typewriter-")
            && !property.starts_with("--markra-")
            && !property.starts_with("--tw-");
    }
    matches!(
        property,
        "color-scheme"
            | "color"
            | "background"
            | "background-color"
            | "background-image"
            | "border"
            | "border-color"
            | "box-shadow"
            | "font-family"
            | "font-feature-settings"
            | "font-kerning"
            | "font-optical-sizing"
            | "font-synthesis"
            | "font-variant"
            | "font-variation-settings"
            | "accent-color"
            | "caret-color"
            | "text-rendering"
    )
}

fn chrome_paint_property_allowed(property: &str) -> bool {
    property.starts_with("--theme-")
        || matches!(
            property,
            "color"
                | "background"
                | "background-color"
                | "background-image"
                | "border"
                | "border-color"
                | "border-top-color"
                | "border-right-color"
                | "border-bottom-color"
                | "border-left-color"
                | "outline-color"
                | "box-shadow"
                | "fill"
                | "stroke"
                | "stroke-width"
                | "text-decoration-color"
        )
}

fn editor_property_allowed(property: &str) -> bool {
    if property.starts_with("--") {
        return true;
    }
    !matches!(
        property,
        "all"
            | "display"
            | "visibility"
            | "opacity"
            | "pointer-events"
            | "z-index"
            | "inset"
            | "inset-block"
            | "inset-block-start"
            | "inset-block-end"
            | "inset-inline"
            | "inset-inline-start"
            | "inset-inline-end"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "animation"
            | "animation-name"
            | "animation-duration"
            | "animation-delay"
            | "animation-direction"
            | "animation-fill-mode"
            | "animation-iteration-count"
            | "animation-play-state"
            | "animation-timing-function"
            | "transform"
            | "translate"
            | "rotate"
            | "scale"
            | "user-select"
            | "-webkit-user-select"
            | "touch-action"
            | "-webkit-app-region"
            | "-webkit-user-modify"
            | "content-visibility"
            | "view-transition-name"
    )
}

fn validate_safe_position(value: &str) -> Result<(), ThemeErrorCode> {
    let mut input = ParserInput::new(value);
    let mut input = Parser::new(&mut input);
    let ident = input
        .expect_ident()
        .map_err(|_| ThemeErrorCode::UnsafeResource)?;
    if !matches_ignore_ascii_case(ident, &["static", "relative"]) || !input.is_exhausted() {
        return Err(ThemeErrorCode::UnsafeResource);
    }
    Ok(())
}

fn matches_ignore_ascii_case(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}

fn error_code(error: ParseError<'_, ThemeErrorCode>) -> ThemeErrorCode {
    match error.kind {
        ParseErrorKind::Custom(code) => code,
        ParseErrorKind::Basic(_) => ThemeErrorCode::InvalidCss,
    }
}
