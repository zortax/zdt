//! The editable model: one parsed pass over the buffer's text at a known revision.
//!
//! The model never owns a mutable document. It is a snapshot — byte ranges into the text it was
//! parsed from, with geometry derived beside them — and an edit is a set of byte replacements
//! addressed at that snapshot. Untouched bytes survive every edit exactly, because nothing ever
//! writes them.

pub mod geometry;
pub mod write;

use std::ops::Range;

use zgui::elements::kurbo;

/// Which editable element a node is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvgTag {
    Path,
    Rect,
    Circle,
    Ellipse,
    Line,
    Polyline,
    Polygon,
}

impl SvgTag {
    /// The tag for `name`, when it is an editable one.
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "path" => Self::Path,
            "rect" => Self::Rect,
            "circle" => Self::Circle,
            "ellipse" => Self::Ellipse,
            "line" => Self::Line,
            "polyline" => Self::Polyline,
            "polygon" => Self::Polygon,
            _ => return None,
        })
    }
}

/// One attribute: its name, its value, and where the value's bytes are.
#[derive(Clone, Debug)]
pub struct SvgAttr {
    pub name: String,
    pub value: String,
    /// The value's bytes, without the quotes.
    pub value_range: Range<usize>,
}

/// One editable element.
#[derive(Clone)]
pub struct SvgNode {
    /// What it is.
    pub tag: SvgTag,
    /// The whole element's bytes. Deleting uses this.
    pub element: Range<usize>,
    /// Where a new attribute can be spliced in: after the last attribute, or after the name.
    pub open_end: usize,
    /// Its attributes, with their value ranges.
    pub attrs: Vec<SvgAttr>,
    /// Its outline in its own coordinates.
    pub local: kurbo::BezPath,
    /// Its own space into the document's: every ancestor transform and its own.
    pub to_doc: kurbo::Affine,
    /// Its own `transform` attribute alone.
    pub own: kurbo::Affine,
    /// Whether its inside catches a press. `fill="none"` does not.
    pub filled: bool,
    /// The fill it is drawn with, when one is written as a presentation attribute.
    pub fill: Option<String>,
    /// The stroke, the same way.
    pub stroke: Option<String>,
    pub stroke_width: f64,
    /// Whether a `style` attribute is on it. Paint edits are refused: a presentation attribute
    /// written beside a style would lose to it and appear to do nothing.
    pub styled: bool,
}

impl SvgNode {
    /// Its outline in document coordinates.
    #[must_use]
    pub fn in_doc(&self) -> kurbo::BezPath {
        let mut path = self.local.clone();
        path.apply_affine(self.to_doc);
        path
    }

    /// Its box in document coordinates.
    #[must_use]
    pub fn bounds(&self) -> kurbo::Rect {
        use kurbo::Shape as _;
        self.in_doc().bounding_box()
    }
}

/// One parsed pass over the text at one revision.
pub struct SvgModel {
    /// The text the ranges address.
    pub source: String,
    /// The revision that text was at.
    pub revision: u64,
    /// The document's own space: minimum x, minimum y, width, height.
    pub view_box: [f64; 4],
    /// The editable elements, in paint order.
    pub nodes: Vec<SvgNode>,
}

/// A change addressed at one snapshot. Refused when the buffer moved past its base.
pub struct SvgEdit {
    /// The revision the ranges address.
    pub base: u64,
    /// The replacements, non-overlapping.
    pub replacements: Vec<(Range<usize>, String)>,
}

impl SvgModel {
    /// Parses `source` as it stands at `revision`. Nothing when it is not well-formed XML.
    #[must_use]
    pub fn parse(source: &str, revision: u64) -> Option<Self> {
        let document = roxmltree::Document::parse(source).ok()?;
        let root = document.root_element();
        if root.tag_name().name() != "svg" {
            return None;
        }

        let view_box = view_box_of(&root, source)?;
        let mut nodes = Vec::new();
        walk(
            root,
            source,
            kurbo::Affine::IDENTITY,
            &Inherited::default(),
            &mut nodes,
        );

        Some(Self {
            source: source.to_owned(),
            revision,
            view_box,
            nodes,
        })
    }

    /// The node at `at`, when the index still names one.
    #[must_use]
    pub fn node(&self, at: usize) -> Option<&SvgNode> {
        self.nodes.get(at)
    }

    /// Replaces or inserts one attribute on one node.
    #[must_use]
    pub fn set_attr(&self, at: usize, name: &str, value: &str) -> Option<SvgEdit> {
        let node = self.node(at)?;
        let value = write::escaped(value);
        let replacement = match node.attrs.iter().find(|held| held.name == name) {
            Some(attr) => (attr.value_range.clone(), value),
            None => (node.open_end..node.open_end, format!(" {name}=\"{value}\"")),
        };
        Some(SvgEdit {
            base: self.revision,
            replacements: vec![replacement],
        })
    }

    /// Replaces or inserts several attributes on one node, as one edit.
    #[must_use]
    pub fn set_attrs(&self, at: usize, values: &[(&str, String)]) -> Option<SvgEdit> {
        let node = self.node(at)?;
        let mut replacements = Vec::new();
        let mut fresh = String::new();
        for (name, value) in values {
            let value = write::escaped(value);
            match node.attrs.iter().find(|held| held.name == *name) {
                Some(attr) => replacements.push((attr.value_range.clone(), value)),
                None => fresh.push_str(&format!(" {name}=\"{value}\"")),
            }
        }
        if !fresh.is_empty() {
            replacements.push((node.open_end..node.open_end, fresh));
        }
        Some(SvgEdit {
            base: self.revision,
            replacements,
        })
    }

    /// Removes one node, whole.
    #[must_use]
    pub fn remove(&self, at: usize) -> Option<SvgEdit> {
        let node = self.node(at)?;
        Some(SvgEdit {
            base: self.revision,
            replacements: vec![(node.element.clone(), String::new())],
        })
    }

    /// `source` with `edit` spliced in, for the render that must not wait for a reparse.
    #[must_use]
    pub fn spliced(&self, edit: &SvgEdit) -> String {
        let mut out = self.source.clone();
        let mut ordered: Vec<&(Range<usize>, String)> = edit.replacements.iter().collect();
        ordered.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, text) in ordered {
            out.replace_range(range.clone(), text);
        }
        out
    }
}

/// What paint an element inherits from its ancestors.
#[derive(Clone, Default)]
struct Inherited {
    fill: Option<String>,
    stroke: Option<String>,
    stroke_width: Option<f64>,
}

/// Subtrees the editor leaves alone: definitions, and content that is not drawn in place.
const OPAQUE: &[&str] = &[
    "defs", "symbol", "clipPath", "mask", "pattern", "marker", "metadata", "title", "desc",
    "style", "script", "text",
];

/// Collects the editable elements under `node`, in paint order.
fn walk(
    node: roxmltree::Node<'_, '_>,
    source: &str,
    to_parent: kurbo::Affine,
    inherited: &Inherited,
    out: &mut Vec<SvgNode>,
) {
    for child in node.children().filter(roxmltree::Node::is_element) {
        let name = child.tag_name().name();
        if OPAQUE.contains(&name) {
            continue;
        }

        let attrs = attrs_of(&child);
        let own = geometry::attr(&attrs, "transform")
            .map(geometry::transform)
            .unwrap_or(kurbo::Affine::IDENTITY);
        let to_doc = to_parent * own;
        let passed = Inherited {
            fill: geometry::attr(&attrs, "fill")
                .map(str::to_owned)
                .or_else(|| inherited.fill.clone()),
            stroke: geometry::attr(&attrs, "stroke")
                .map(str::to_owned)
                .or_else(|| inherited.stroke.clone()),
            stroke_width: geometry::attr(&attrs, "stroke-width")
                .and_then(|held| held.trim().parse().ok())
                .or(inherited.stroke_width),
        };

        if let Some(tag) = SvgTag::of(name) {
            if let Some(local) = geometry::outline(tag, &attrs) {
                let filled = passed.fill.as_deref() != Some("none")
                    && !matches!(tag, SvgTag::Line | SvgTag::Polyline);
                out.push(SvgNode {
                    tag,
                    element: child.range(),
                    open_end: open_end_of(&child, source),
                    styled: geometry::attr(&attrs, "style").is_some(),
                    filled,
                    fill: passed.fill.clone(),
                    stroke: passed.stroke.clone(),
                    stroke_width: passed.stroke_width.unwrap_or(1.0),
                    local,
                    to_doc,
                    own,
                    attrs,
                });
            }
        } else {
            walk(child, source, to_doc, &passed, out);
        }
    }
}

/// The attributes with their value ranges.
fn attrs_of(node: &roxmltree::Node<'_, '_>) -> Vec<SvgAttr> {
    node.attributes()
        .map(|attr| SvgAttr {
            name: attr.name().to_owned(),
            value: attr.value().to_owned(),
            value_range: attr.range_value(),
        })
        .collect()
}

/// Where a new attribute goes: after the last attribute, or straight after the tag's name.
fn open_end_of(node: &roxmltree::Node<'_, '_>, source: &str) -> usize {
    if let Some(last) = node.attributes().map(|attr| attr.range().end).max() {
        return last;
    }
    let start = node.range().start;
    let name_end = start + 1 + node.tag_name().name().len();
    // Past a namespace prefix, when one is written.
    source[name_end..]
        .find(|held: char| held.is_ascii_whitespace() || held == '>' || held == '/')
        .map_or(name_end, |offset| name_end + offset)
}

/// The document's own space, from `viewBox` or from its width and height.
fn view_box_of(root: &roxmltree::Node<'_, '_>, _source: &str) -> Option<[f64; 4]> {
    if let Some(value) = root.attribute("viewBox") {
        let numbers: Vec<f64> = value
            .split(|held: char| held.is_ascii_whitespace() || held == ',')
            .filter(|held| !held.is_empty())
            .filter_map(|held| held.parse().ok())
            .collect();
        if let [x, y, width, height] = numbers[..]
            && width > 0.0
            && height > 0.0
        {
            return Some([x, y, width, height]);
        }
        return None;
    }
    let length = |name: &str| -> Option<f64> {
        root.attribute(name)?
            .trim()
            .trim_end_matches("px")
            .trim()
            .parse()
            .ok()
    };
    let (width, height) = (length("width")?, length("height")?);
    (width > 0.0 && height > 0.0).then_some([0.0, 0.0, width, height])
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">
  <!-- a comment that must survive -->
  <rect x="10" y="10" width="30" height="20" fill="#ff0000"/>
  <g transform="translate(50 0)">
    <circle cx="10" cy="25" r="8" fill="none" stroke="#00ff00"/>
  </g>
  <path d="M 5 5 L 20 5" stroke="#0000ff"/>
</svg>
"##;

    fn model() -> SvgModel {
        SvgModel::parse(PAGE, 7).expect("a readable page")
    }

    #[test]
    fn the_page_parses_into_editable_nodes() {
        let held = model();
        assert_eq!(held.view_box, [0.0, 0.0, 100.0, 50.0]);
        assert_eq!(held.nodes.len(), 3);
        assert_eq!(held.nodes[0].tag, SvgTag::Rect);
        assert_eq!(held.nodes[1].tag, SvgTag::Circle);
        assert_eq!(held.nodes[2].tag, SvgTag::Path);
        assert_eq!(held.revision, 7);
    }

    #[test]
    fn ancestor_transforms_reach_the_node() {
        let held = model();
        let circle = &held.nodes[1];
        // translate(50 0) carries the local centre (10, 25) to (60, 25).
        let centre = circle.to_doc * kurbo::Point::new(10.0, 25.0);
        assert_eq!(centre, kurbo::Point::new(60.0, 25.0));
        assert!(!circle.filled, "fill=none catches no press");
    }

    #[test]
    fn replacing_an_attribute_touches_its_bytes_alone() {
        let held = model();
        let edit = held.set_attr(0, "x", "42").expect("the rect is there");
        let out = held.spliced(&edit);
        assert!(out.contains(r#"<rect x="42" y="10""#));
        assert!(out.contains("a comment that must survive"));
        // Everything outside the one value is byte-for-byte the page.
        assert_eq!(out.len(), PAGE.len());
    }

    #[test]
    fn a_missing_attribute_is_spliced_in_after_the_last() {
        let held = model();
        let edit = held
            .set_attr(2, "transform", "translate(1 2)")
            .expect("the path is there");
        let out = held.spliced(&edit);
        assert!(out.contains(r##"stroke="#0000ff" transform="translate(1 2)"/>"##));
    }

    #[test]
    fn several_values_land_as_one_edit() {
        let held = model();
        let edit = held
            .set_attrs(
                0,
                &[("x", "1".into()), ("y", "2".into()), ("rx", "3".into())],
            )
            .expect("the rect is there");
        let out = held.spliced(&edit);
        assert!(out.contains(r#"x="1""#));
        assert!(out.contains(r#"y="2""#));
        assert!(out.contains(r#"rx="3""#));
    }

    #[test]
    fn removing_a_node_takes_the_whole_element() {
        let held = model();
        let edit = held.remove(1).expect("the circle is there");
        let out = held.spliced(&edit);
        assert!(!out.contains("circle"));
        assert!(out.contains("<g transform"), "the group stays");
        assert!(SvgModel::parse(&out, 8).is_some(), "still well-formed");
    }

    #[test]
    fn what_is_not_a_drawing_parses_to_nothing() {
        assert!(SvgModel::parse("just words", 0).is_none());
        assert!(SvgModel::parse("<svg viewBox='0 0 0 0'/>", 0).is_none());
        assert!(SvgModel::parse("<html xmlns='x'><p/></html>", 0).is_none());
    }

    #[test]
    fn a_sized_document_without_a_view_box_still_answers_one() {
        let held = SvgModel::parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24px" height="12"><rect width="4" height="4"/></svg>"#,
            0,
        )
        .expect("readable");
        assert_eq!(held.view_box, [0.0, 0.0, 24.0, 12.0]);
    }

    #[test]
    fn definitions_are_left_alone() {
        let held = SvgModel::parse(
            r#"<svg xmlns="x" viewBox="0 0 10 10"><defs><rect width="4" height="4"/></defs><rect width="2" height="2"/></svg>"#,
            0,
        )
        .expect("readable");
        assert_eq!(held.nodes.len(), 1);
    }
}
