//! Elements of an app's window found through accessibility rather than by look: what a test
//! asks for, the kinds of element it can name, and which elements of the window answer.

use jevision_l10n::tr_in;
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::Rect;

/// An element of the window, found by what accessibility says about it. An empty field is not
/// asked about; a test gives at least one of `id`, `role` and `title`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ElementQuery {
    /// The accessibility identifier the app gave the element.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The kind of element: a name of [`ELEMENT_ROLES`], such as `button`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// The label, matched exactly: see [`ElementNode::label`].
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Which of the matching elements, from 0, in the order they come in the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// What a test expects of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementState {
    /// There is such an element in the window.
    Present,
    /// There is none.
    Absent,
    /// It can be used.
    Enabled,
    /// It is dimmed and can't be used.
    Disabled,
    /// Selected, or checked for a checkbox, radio button or switch.
    Selected,
    NotSelected,
    /// Its value, such as the text of a field, is exactly this.
    ValueEquals(String),
    ValueContains(String),
    /// Its label is exactly this.
    TitleEquals(String),
    TitleContains(String),
}

/// The names of the states in a test file, in the order the editor offers them.
pub const ELEMENT_STATES: &[&str] = &[
    "present",
    "absent",
    "enabled",
    "disabled",
    "selected",
    "notSelected",
    "valueEquals",
    "valueContains",
    "titleEquals",
    "titleContains",
];

impl ElementState {
    /// The state named `name` in a test file; `to` is the text of the ones that compare text.
    pub fn named(name: &str, to: String) -> Option<Self> {
        Some(match name {
            "present" => Self::Present,
            "absent" => Self::Absent,
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "selected" => Self::Selected,
            "notSelected" => Self::NotSelected,
            "valueEquals" => Self::ValueEquals(to),
            "valueContains" => Self::ValueContains(to),
            "titleEquals" => Self::TitleEquals(to),
            "titleContains" => Self::TitleContains(to),
            _ => return None,
        })
    }

    /// Whether the state named `name` compares text, so a test gives it `to`.
    pub fn compares_text(name: &str) -> bool {
        name.starts_with("value") || name.starts_with("title")
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Selected => "selected",
            Self::NotSelected => "notSelected",
            Self::ValueEquals(_) => "valueEquals",
            Self::ValueContains(_) => "valueContains",
            Self::TitleEquals(_) => "titleEquals",
            Self::TitleContains(_) => "titleContains",
        }
    }

    /// The text the state compares with.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::ValueEquals(t)
            | Self::ValueContains(t)
            | Self::TitleEquals(t)
            | Self::TitleContains(t) => Some(t),
            _ => None,
        }
    }

    /// The same state with `text` in place of its text.
    pub fn with_text(&self, text: String) -> Self {
        Self::named(self.name(), text).expect("a known name")
    }
}

/// An element and what is expected of it: `{"role": "button", "title": "Save", "is": "enabled"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementCheck {
    pub element: ElementQuery,
    pub is: ElementState,
}

impl Serialize for ElementCheck {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let e = &self.element;
        let mut map = s.serialize_map(None)?;
        for (key, value) in [("id", &e.id), ("role", &e.role), ("title", &e.title)] {
            if !value.is_empty() {
                map.serialize_entry(key, value)?;
            }
        }
        if let Some(index) = e.index {
            map.serialize_entry("index", &index)?;
        }
        map.serialize_entry("is", self.is.name())?;
        if let Some(to) = self.is.text() {
            map.serialize_entry("to", to)?;
        }
        map.end()
    }
}

/// An element of the window as the platform reports it, in the order of a walk through the
/// window that visits an element before what is inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementNode {
    /// The accessibility role, such as `AXButton`.
    pub role: String,
    /// The accessibility subrole, such as `AXSwitch`; empty when there is none.
    pub subrole: String,
    /// The accessibility identifier.
    pub id: String,
    pub title: String,
    pub description: String,
    /// The value as text: what a field holds, `1` for a checked checkbox.
    pub value: String,
    /// The grey hint of an empty text field.
    pub placeholder: String,
    pub enabled: bool,
    /// Whether accessibility says the element is selected, such as a row of a list.
    pub selected: bool,
    /// Where the element is, in fractions of the window.
    pub frame: Rect,
}

impl ElementNode {
    /// What people see the element called: its title, else its description, else the text of
    /// static text, else the hint of an empty field.
    pub fn label(&self) -> &str {
        let text = (self.role == "AXStaticText").then_some(&self.value);
        [
            Some(&self.title),
            Some(&self.description),
            text,
            Some(&self.placeholder),
        ]
        .into_iter()
        .flatten()
        .find(|t| !t.is_empty())
        .map_or("", String::as_str)
    }

    /// Selected, or checked for a checkbox, radio button or switch.
    pub fn is_selected(&self) -> bool {
        self.selected
            || (matches!(self.role.as_str(), "AXCheckBox" | "AXRadioButton") && self.value == "1")
    }

    /// The element as a test would ask for it: its kind, id and label.
    pub fn as_query(&self) -> ElementQuery {
        ElementQuery {
            id: self.id.clone(),
            role: ElementRole::of(self).map_or_else(
                || self.role.trim_start_matches("AX").to_owned(),
                |r| r.name.to_owned(),
            ),
            title: self.label().to_owned(),
            index: None,
        }
    }
}

/// A kind of element a test can name, and the accessibility role it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRole {
    /// The name in a test file, such as `textField`.
    pub name: &'static str,
    role: &'static str,
    /// Empty when any subrole will do.
    subrole: &'static str,
    key: &'static str,
}

const fn role(
    name: &'static str,
    role: &'static str,
    subrole: &'static str,
    key: &'static str,
) -> ElementRole {
    ElementRole {
        name,
        role,
        subrole,
        key,
    }
}

/// The kinds of element a test can name, in the order the editor offers them.
pub const ELEMENT_ROLES: &[ElementRole] = &[
    role("button", "AXButton", "", "element.role.button"),
    role("link", "AXLink", "", "element.role.link"),
    role("checkBox", "AXCheckBox", "", "element.role.checkBox"),
    role("switch", "AXCheckBox", "AXSwitch", "element.role.switch"),
    role(
        "radioButton",
        "AXRadioButton",
        "",
        "element.role.radioButton",
    ),
    role("radioGroup", "AXRadioGroup", "", "element.role.radioGroup"),
    role("textField", "AXTextField", "", "element.role.textField"),
    role(
        "secureTextField",
        "AXTextField",
        "AXSecureTextField",
        "element.role.secureTextField",
    ),
    role(
        "searchField",
        "AXTextField",
        "AXSearchField",
        "element.role.searchField",
    ),
    role("textArea", "AXTextArea", "", "element.role.textArea"),
    role("comboBox", "AXComboBox", "", "element.role.comboBox"),
    role(
        "popUpButton",
        "AXPopUpButton",
        "",
        "element.role.popUpButton",
    ),
    role("menuButton", "AXMenuButton", "", "element.role.menuButton"),
    role("menuItem", "AXMenuItem", "", "element.role.menuItem"),
    role("slider", "AXSlider", "", "element.role.slider"),
    role("stepper", "AXIncrementor", "", "element.role.stepper"),
    role("staticText", "AXStaticText", "", "element.role.staticText"),
    role("image", "AXImage", "", "element.role.image"),
    role("tabGroup", "AXTabGroup", "", "element.role.tabGroup"),
    role("list", "AXList", "", "element.role.list"),
    role("table", "AXTable", "", "element.role.table"),
    role("outline", "AXOutline", "", "element.role.outline"),
    role("row", "AXRow", "", "element.role.row"),
    role("cell", "AXCell", "", "element.role.cell"),
    role(
        "disclosureTriangle",
        "AXDisclosureTriangle",
        "",
        "element.role.disclosureTriangle",
    ),
    role(
        "progressIndicator",
        "AXProgressIndicator",
        "",
        "element.role.progressIndicator",
    ),
    role(
        "busyIndicator",
        "AXBusyIndicator",
        "",
        "element.role.busyIndicator",
    ),
    role("colorWell", "AXColorWell", "", "element.role.colorWell"),
    role("scrollArea", "AXScrollArea", "", "element.role.scrollArea"),
    role("group", "AXGroup", "", "element.role.group"),
    role("toolbar", "AXToolbar", "", "element.role.toolbar"),
    role("sheet", "AXSheet", "", "element.role.sheet"),
    role("window", "AXWindow", "", "element.role.window"),
];

impl ElementRole {
    /// The kind named `name` in a test file.
    pub fn named(name: &str) -> Option<&'static Self> {
        ELEMENT_ROLES.iter().find(|r| r.name == name)
    }

    /// The most precise kind `node` is: a switch rather than a checkbox.
    pub fn of(node: &ElementNode) -> Option<&'static Self> {
        let fits = |r: &&Self| r.role == node.role;
        let mut kinds = ELEMENT_ROLES.iter().filter(fits);
        kinds
            .clone()
            .find(|r| !r.subrole.is_empty() && r.subrole == node.subrole)
            .or_else(|| kinds.find(|r| r.subrole.is_empty()))
    }

    /// Whether `node` is of this kind; a switch is a checkbox too.
    pub fn fits(&self, node: &ElementNode) -> bool {
        self.role == node.role && (self.subrole.is_empty() || self.subrole == node.subrole)
    }

    /// What people call this kind in `lang`, in lower case: `text field`.
    pub fn label_in(&self, lang: &str) -> String {
        tr_in!(lang, self.key)
    }
}

/// Why no single element answers a query.
#[derive(Debug, Clone, PartialEq)]
pub enum Missing<'n> {
    /// None matches; the elements most like the one asked for, to help put the query right.
    NotFound(Vec<&'n ElementNode>),
    /// Several match and the query has no index to pick one.
    Ambiguous(Vec<&'n ElementNode>),
}

/// How many similar elements a failure lists at most.
const SIMILAR: usize = 5;

impl ElementQuery {
    /// Asks about nothing that tells elements apart: no id, role or title.
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.role.is_empty() && self.title.is_empty()
    }

    /// Whether `node` has the id, kind and label asked for; the index is not looked at.
    pub fn matches(&self, node: &ElementNode) -> bool {
        (self.id.is_empty() || node.id == self.id)
            && (self.title.is_empty() || node.label() == self.title)
            && (self.role.is_empty()
                || ElementRole::named(&self.role).is_some_and(|r| r.fits(node)))
    }

    /// The elements of `nodes` that match, in their order, whatever the index.
    pub fn matching<'n>(&self, nodes: &'n [ElementNode]) -> Vec<&'n ElementNode> {
        nodes.iter().filter(|n| self.matches(n)).collect()
    }

    /// The one element the query means: the only match, or the match at its index.
    pub fn find<'n>(&self, nodes: &'n [ElementNode]) -> Result<&'n ElementNode, Missing<'n>> {
        let found = self.matching(nodes);
        match (self.index, &found[..]) {
            (Some(i), _) => found
                .get(i as usize)
                .copied()
                .ok_or_else(|| Missing::NotFound(self.similar(nodes))),
            (None, [one]) => Ok(one),
            (None, []) => Err(Missing::NotFound(self.similar(nodes))),
            (None, _) => Err(Missing::Ambiguous(found)),
        }
    }

    /// Elements like the one asked for, when it isn't there: the matches when only the index is
    /// too big, else those with the same id or a label like the title (of the kind, when some
    /// are), else those of the kind.
    pub fn similar<'n>(&self, nodes: &'n [ElementNode]) -> Vec<&'n ElementNode> {
        let found = self.matching(nodes);
        if !found.is_empty() {
            return found.into_iter().take(SIMILAR).collect();
        }
        let title = self.title.to_lowercase();
        let near = |n: &&ElementNode| {
            (!self.id.is_empty() && n.id == self.id)
                || (!title.is_empty() && {
                    let label = n.label().to_lowercase();
                    !label.is_empty() && (label.contains(&title) || title.contains(&label))
                })
        };
        let kind = ElementRole::named(&self.role);
        let mut like: Vec<_> = nodes.iter().filter(near).collect();
        if let Some(kind) = kind
            && like.iter().any(|n| kind.fits(n))
        {
            like.retain(|n| kind.fits(n));
        }
        like.truncate(SIMILAR);
        if like.is_empty()
            && let Some(kind) = kind
        {
            like = nodes
                .iter()
                .filter(|n| kind.fits(n) && !n.label().is_empty())
                .take(SIMILAR)
                .collect();
        }
        like
    }
}

/// What the window says about an [`ElementCheck`].
#[derive(Debug, Clone, PartialEq)]
pub struct Observation<'n> {
    pub holds: bool,
    /// The element looked at; `None` when there is no such element.
    pub element: Option<&'n ElementNode>,
    /// What the element is like, in the terms of the check: `Disabled` for `enabled`, the
    /// value it has for `valueEquals` and `valueContains`, `Absent` when it isn't there.
    pub now: ElementState,
}

impl ElementCheck {
    /// Looks at the elements of the window. `Err` holds the matches when several answer a
    /// check that needs one element and the query has no index to pick it.
    pub fn observe<'n>(
        &self,
        nodes: &'n [ElementNode],
    ) -> Result<Observation<'n>, Vec<&'n ElementNode>> {
        use ElementState::*;
        let presence = matches!(self.is, Present | Absent);
        let found = self.element.matching(nodes);
        let element = match (self.element.index, &found[..]) {
            (Some(i), _) => found.get(i as usize).copied(),
            (None, []) => None,
            (None, [one, ..]) if presence || found.len() == 1 => Some(*one),
            (None, _) => return Err(found),
        };
        let now = match (element, &self.is) {
            (None, _) => Absent,
            (Some(_), Present | Absent) => Present,
            (Some(e), Enabled | Disabled) => {
                if e.enabled {
                    Enabled
                } else {
                    Disabled
                }
            }
            (Some(e), Selected | NotSelected) => {
                if e.is_selected() {
                    Selected
                } else {
                    NotSelected
                }
            }
            (Some(e), ValueEquals(_) | ValueContains(_)) => ValueEquals(e.value.clone()),
            (Some(e), TitleEquals(_) | TitleContains(_)) => TitleEquals(e.label().to_owned()),
        };
        let holds = match (&self.is, &now) {
            (ValueEquals(want), ValueEquals(have)) | (TitleEquals(want), TitleEquals(have)) => {
                want == have
            }
            (ValueContains(want), ValueEquals(have)) | (TitleContains(want), TitleEquals(have)) => {
                have.contains(want.as_str())
            }
            (want, have) => want == have,
        };
        Ok(Observation {
            holds,
            element,
            now,
        })
    }
}

/// The element `query` asks for in `lang`: `button “Save” with id “save” (index 1)`.
pub fn describe_element_in(lang: &str, query: &ElementQuery) -> String {
    let mut text = match ElementRole::named(&query.role) {
        Some(kind) => kind.label_in(lang),
        None if query.role.is_empty() => tr_in!(lang, "element.any"),
        None => query.role.clone(),
    };
    if !query.title.is_empty() {
        text = tr_in!(lang, "element.withTitle", text, &query.title);
    }
    if !query.id.is_empty() {
        text = tr_in!(lang, "element.withId", text, &query.id);
    }
    if let Some(index) = query.index {
        text = tr_in!(lang, "element.withIndex", text, index.to_string());
    }
    text
}

/// The elements in `lang`, separated by commas.
pub fn describe_elements_in(lang: &str, nodes: &[&ElementNode]) -> String {
    nodes
        .iter()
        .map(|n| describe_element_in(lang, &n.as_query()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A state in `lang`: `enabled`, `value “Alice”`.
pub fn describe_state_in(lang: &str, state: &ElementState) -> String {
    use ElementState::*;
    match state {
        Present => tr_in!(lang, "element.state.present"),
        Absent => tr_in!(lang, "element.state.absent"),
        Enabled => tr_in!(lang, "element.state.enabled"),
        Disabled => tr_in!(lang, "element.state.disabled"),
        Selected => tr_in!(lang, "element.state.selected"),
        NotSelected => tr_in!(lang, "element.state.notSelected"),
        ValueEquals(t) => tr_in!(lang, "element.state.valueEquals", t),
        ValueContains(t) => tr_in!(lang, "element.state.valueContains", t),
        TitleEquals(t) => tr_in!(lang, "element.state.titleEquals", t),
        TitleContains(t) => tr_in!(lang, "element.state.titleContains", t),
    }
}

/// An element as written in the test, in no language, for a title or the name of a condition:
/// `button “Save” #save [1]`.
pub fn element_name(e: &ElementQuery) -> String {
    let mut parts = Vec::new();
    if !e.role.is_empty() {
        parts.push(e.role.clone());
    }
    if !e.title.is_empty() {
        parts.push(format!("“{}”", e.title));
    }
    if !e.id.is_empty() {
        parts.push(format!("#{}", e.id));
    }
    if let Some(index) = e.index {
        parts.push(format!("[{index}]"));
    }
    parts.join(" ")
}

/// A check as written in the test, for the name of the way an `if` took:
/// `button “Save” enabled`, `textField #name valueEquals “Alice”`.
pub fn element_check_name(check: &ElementCheck) -> String {
    let mut name = format!("{} {}", element_name(&check.element), check.is.name());
    if let Some(to) = check.is.text() {
        name += &format!(" “{to}”");
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(role: &str, subrole: &str, id: &str, title: &str) -> ElementNode {
        ElementNode {
            role: role.to_owned(),
            subrole: subrole.to_owned(),
            id: id.to_owned(),
            title: title.to_owned(),
            description: String::new(),
            value: String::new(),
            placeholder: String::new(),
            enabled: true,
            selected: false,
            frame: Rect {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.1,
            },
        }
    }

    /// A window with a toolbar, a form and a list, in the order of a walk through it.
    fn window() -> Vec<ElementNode> {
        let mut name = node("AXTextField", "", "name", "");
        name.placeholder = "Your name".into();
        name.value = "Alice".into();
        let mut password = node("AXTextField", "AXSecureTextField", "password", "");
        password.placeholder = "Password".into();
        let mut agree = node("AXCheckBox", "", "agree", "I agree");
        agree.value = "1".into();
        let mut dark = node("AXCheckBox", "AXSwitch", "", "Dark mode");
        dark.value = "0".into();
        let mut status = node("AXStaticText", "", "status", "");
        status.value = "3 items saved".into();
        let mut save = node("AXButton", "", "save", "Save");
        save.enabled = false;
        let mut icon = node("AXButton", "", "", "");
        icon.description = "Delete".into();
        let mut second = node("AXRow", "", "", "");
        second.selected = true;
        vec![
            node("AXToolbar", "", "", ""),
            icon,
            name,
            password,
            agree,
            dark,
            status,
            save,
            node("AXButton", "", "cancel", "Cancel"),
            node("AXRow", "", "", ""),
            second,
            node("AXRow", "", "", ""),
        ]
    }

    fn query(id: &str, role: &str, title: &str, index: Option<u32>) -> ElementQuery {
        ElementQuery {
            id: id.into(),
            role: role.into(),
            title: title.into(),
            index,
        }
    }

    fn check(q: ElementQuery, is: ElementState) -> ElementCheck {
        ElementCheck { element: q, is }
    }

    #[test]
    fn finds_by_id_role_or_label() {
        let w = window();
        assert_eq!(w[7], *query("save", "", "", None).find(&w).unwrap());
        assert_eq!(w[7], *query("", "button", "Save", None).find(&w).unwrap());
        // The label falls back to the description, the text of static text and the hint.
        assert_eq!(w[1], *query("", "", "Delete", None).find(&w).unwrap());
        assert_eq!(
            w[6],
            *query("", "", "3 items saved", None).find(&w).unwrap()
        );
        assert_eq!(
            w[2],
            *query("", "textField", "Your name", None).find(&w).unwrap()
        );
        // The label is matched exactly.
        assert!(query("", "", "save", None).find(&w).is_err());
    }

    #[test]
    fn a_kind_with_a_subrole_is_the_narrower_one() {
        let w = window();
        // A password field is a text field too, but a text field is not a password field.
        assert_eq!(2, query("", "textField", "", None).matching(&w).len());
        assert_eq!(
            w[3],
            *query("", "secureTextField", "", None).find(&w).unwrap()
        );
        assert_eq!(w[5], *query("", "switch", "", None).find(&w).unwrap());
        assert_eq!(2, query("", "checkBox", "", None).matching(&w).len());
        assert_eq!("switch", w[5].as_query().role);
        assert_eq!("checkBox", w[4].as_query().role);
        assert_eq!("toolbar", w[0].as_query().role);
        // A kind a test can't name is shown by its accessibility role.
        assert_eq!("Splitter", node("AXSplitter", "", "", "").as_query().role);
    }

    #[test]
    fn several_matches_need_an_index() {
        let w = window();
        let rows = query("", "row", "", None);
        match rows.find(&w) {
            Err(Missing::Ambiguous(found)) => assert_eq!(3, found.len()),
            other => panic!("expected several rows, got {other:?}"),
        }
        assert_eq!(w[10], *query("", "row", "", Some(1)).find(&w).unwrap());
        // An index past the matches lists them as the similar ones.
        match query("", "row", "", Some(3)).find(&w) {
            Err(Missing::NotFound(like)) => assert_eq!(3, like.len()),
            other => panic!("expected no row 3, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_element_comes_with_similar_ones() {
        let w = window();
        let like = |q: ElementQuery| match q.find(&w) {
            Err(Missing::NotFound(like)) => like.iter().map(|n| n.as_query()).collect::<Vec<_>>(),
            other => panic!("expected nothing found, got {other:?}"),
        };
        // Same id, another kind.
        assert_eq!(vec![w[7].as_query()], like(query("save", "link", "", None)));
        // A label in other case.
        assert_eq!(
            vec![w[7].as_query()],
            like(query("", "button", "save", None))
        );
        // Nothing near: the elements of the kind that have a label.
        assert_eq!(
            vec![w[1].as_query(), w[7].as_query(), w[8].as_query()],
            like(query("", "button", "Open", None))
        );
        assert!(like(query("", "slider", "", None)).is_empty());
    }

    #[test]
    fn checks_presence_without_needing_one_element() {
        let w = window();
        let rows = query("", "row", "", None);
        assert!(
            check(rows.clone(), ElementState::Present)
                .observe(&w)
                .unwrap()
                .holds
        );
        assert!(!check(rows, ElementState::Absent).observe(&w).unwrap().holds);
        let fourth = query("", "row", "", Some(3));
        let seen = check(fourth, ElementState::Absent).observe(&w).unwrap();
        assert!(seen.holds && seen.element.is_none());
        let seen = check(query("open", "", "", None), ElementState::Present)
            .observe(&w)
            .unwrap();
        assert_eq!((false, ElementState::Absent), (seen.holds, seen.now));
    }

    #[test]
    fn checks_the_state_of_one_element() {
        use ElementState::*;
        let w = window();
        let holds = |q: ElementQuery, is: ElementState| {
            let seen = check(q, is).observe(&w).unwrap();
            (seen.holds, seen.now)
        };
        assert_eq!(
            (false, Disabled),
            holds(query("save", "", "", None), Enabled)
        );
        assert_eq!(
            (true, Disabled),
            holds(query("save", "", "", None), Disabled)
        );
        assert_eq!(
            (true, Enabled),
            holds(query("cancel", "", "", None), Enabled)
        );
        // Checked is selected for a checkbox; a switch that is off is not.
        assert_eq!(
            (true, Selected),
            holds(query("agree", "", "", None), Selected)
        );
        assert_eq!(
            (true, NotSelected),
            holds(query("", "switch", "", None), NotSelected)
        );
        assert_eq!(
            (true, Selected),
            holds(query("", "row", "", Some(1)), Selected)
        );
        let name = || query("name", "", "", None);
        assert_eq!(
            (true, ValueEquals("Alice".into())),
            holds(name(), ValueEquals("Alice".into()))
        );
        assert_eq!(
            (false, ValueEquals("Alice".into())),
            holds(name(), ValueEquals("Bob".into()))
        );
        assert!(holds(name(), ValueContains("lic".into())).0);
        let status = || query("status", "", "", None);
        assert!(holds(status(), TitleContains("saved".into())).0);
        assert_eq!(
            (false, TitleEquals("3 items saved".into())),
            holds(status(), TitleEquals("saved".into()))
        );
        // An element that isn't there has no state to compare.
        assert_eq!(
            (false, Absent),
            holds(query("open", "", "", None), Disabled)
        );
    }

    #[test]
    fn a_state_of_one_of_several_is_ambiguous() {
        let w = window();
        let found = check(query("", "row", "", None), ElementState::Selected)
            .observe(&w)
            .unwrap_err();
        assert_eq!(3, found.len());
    }

    #[test]
    fn a_check_is_written_flat_with_its_text_only_when_it_compares_text() {
        let enabled = check(query("save", "button", "", None), ElementState::Enabled);
        assert_eq!(
            r#"{"id":"save","role":"button","is":"enabled"}"#,
            serde_json::to_string(&enabled).unwrap()
        );
        let value = check(
            query("", "textField", "", Some(0)),
            ElementState::ValueEquals(String::new()),
        );
        assert_eq!(
            r#"{"role":"textField","index":0,"is":"valueEquals","to":""}"#,
            serde_json::to_string(&value).unwrap()
        );
        assert_eq!("textField [0] valueEquals “”", element_check_name(&value));
    }

    #[test]
    fn describes_elements_and_states_in_words() {
        let q = query("save", "button", "Save", Some(1));
        assert_eq!(
            "button “Save” with id “save” (index 1)",
            describe_element_in("en", &q)
        );
        assert_eq!(
            "кнопка «Save» с id «save» (индекс 1)",
            describe_element_in("ru", &q)
        );
        assert_eq!(
            "element “Save”",
            describe_element_in("en", &query("", "", "Save", None))
        );
        assert_eq!(
            "value containing “Al”",
            describe_state_in("en", &ElementState::ValueContains("Al".into()))
        );
        for kind in ELEMENT_ROLES {
            assert_ne!(
                kind.key,
                kind.label_in("en"),
                "{} has no English name",
                kind.name
            );
            assert_ne!(
                kind.key,
                kind.label_in("ru"),
                "{} has no Russian name",
                kind.name
            );
        }
    }
}
