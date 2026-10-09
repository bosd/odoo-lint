//! A small mutable XML tree and the subset of XPath 1.0 that Odoo views use,
//! to apply view inheritance the way Odoo does (`apply_inheritance_specs`):
//! locate the node of each spec and insert, replace, move or change it.
//!
//! The XPath subset: location paths with `/`, `//`, `.`, `..`, `*`, `@attr`,
//! `text()`, `node()`; predicates with positions, comparisons, `and`, `or`,
//! `not()`, `contains()`, `starts-with()`, `normalize-space()`, `string()`,
//! `local-name()`, `name()`, `count()`, `position()`, `last()` and Odoo's
//! `hasclass()`; unions with `|`. An expression outside the subset does not
//! parse, and its spec is not checked.

pub type NodeId = usize;

#[derive(Debug, Clone)]
enum Kind {
    /// The document node, above the root element.
    Document,
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

#[derive(Debug, Clone)]
struct NodeData {
    kind: Kind,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

/// An XML tree with stable node ids; removed nodes stay in the arena.
#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<NodeData>,
}

impl Tree {
    /// A tree whose root element is a copy of `root`.
    pub fn from_node(root: roxmltree::Node) -> Self {
        let mut tree = Tree {
            nodes: vec![NodeData {
                kind: Kind::Document,
                parent: None,
                children: Vec::new(),
            }],
        };
        let id = tree.import(root);
        tree.append(0, id);
        tree
    }

    /// Copies `node` (an element or text) and its subtree into the arena,
    /// detached.
    pub fn import(&mut self, node: roxmltree::Node) -> NodeId {
        let kind = if node.is_text() {
            Kind::Text(node.text().unwrap_or_default().to_string())
        } else {
            Kind::Element {
                tag: node.tag_name().name().to_string(),
                attrs: node
                    .attributes()
                    .map(|a| (a.name().to_string(), a.value().to_string()))
                    .collect(),
            }
        };
        let id = self.nodes.len();
        self.nodes.push(NodeData {
            kind,
            parent: None,
            children: Vec::new(),
        });
        for child in node.children().filter(|c| c.is_element() || c.is_text()) {
            let child_id = self.import(child);
            self.append(id, child_id);
        }
        id
    }

    /// The root element.
    pub fn root(&self) -> Option<NodeId> {
        self.nodes[0].children.iter().copied().find(|c| self.is_element(*c))
    }

    pub fn is_element(&self, node: NodeId) -> bool {
        matches!(self.nodes[node].kind, Kind::Element { .. })
    }

    pub fn tag(&self, node: NodeId) -> &str {
        match &self.nodes[node].kind {
            Kind::Element { tag, .. } => tag,
            _ => "",
        }
    }

    pub fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        match &self.nodes[node].kind {
            Kind::Element { attrs, .. } => attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn attrs(&self, node: NodeId) -> &[(String, String)] {
        match &self.nodes[node].kind {
            Kind::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn set_tag(&mut self, node: NodeId, new: &str) {
        if let Kind::Element { tag, .. } = &mut self.nodes[node].kind {
            *tag = new.to_string();
        }
    }

    pub fn set_attr(&mut self, node: NodeId, name: &str, value: Option<String>) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[node].kind {
            attrs.retain(|(n, _)| n != name);
            if let Some(value) = value {
                attrs.push((name.to_string(), value));
            }
        }
    }

    pub fn children(&self, node: NodeId) -> &[NodeId] {
        &self.nodes[node].children
    }

    pub fn element_children(&self, node: NodeId) -> Vec<NodeId> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .filter(|c| self.is_element(*c))
            .collect()
    }

    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.nodes[node].parent
    }

    fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    fn insert_at(&mut self, parent: NodeId, index: usize, child: NodeId) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        let index = index.min(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(index, child);
    }

    fn detach(&mut self, node: NodeId) {
        if let Some(parent) = self.nodes[node].parent.take() {
            self.nodes[parent].children.retain(|c| *c != node);
        }
    }

    /// The text of a node: concatenated descendant text.
    pub fn string_value(&self, node: NodeId) -> String {
        match &self.nodes[node].kind {
            Kind::Text(text) => text.clone(),
            _ => self.nodes[node]
                .children
                .iter()
                .map(|c| self.string_value(*c))
                .collect(),
        }
    }

    /// The descendants of `node` in document order, itself excluded.
    pub fn descendants(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.nodes[node].children.iter().rev().copied().collect();
        while let Some(current) = stack.pop() {
            out.push(current);
            stack.extend(self.nodes[current].children.iter().rev().copied());
        }
        out
    }

    /// Document-order rank of every attached node.
    fn order(&self) -> Vec<usize> {
        let mut rank = vec![usize::MAX; self.nodes.len()];
        rank[0] = 0;
        for (i, node) in self.descendants(0).into_iter().enumerate() {
            rank[node] = i + 1;
        }
        rank
    }
}

// --- XPath ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Slash,
    DoubleSlash,
    Dot,
    DotDot,
    At,
    Star,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Comma,
    Pipe,
    Op(String),
    Name(String),
    Str(String),
    Num(f64),
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                tokens.push(Token::DoubleSlash);
                i += 2;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '.' if chars.get(i + 1) == Some(&'.') => {
                tokens.push(Token::DotDot);
                i += 2;
            }
            '.' if !chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                tokens.push(Token::Dot);
                i += 1;
            }
            '@' => {
                tokens.push(Token::At);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '|' => {
                tokens.push(Token::Pipe);
                i += 1;
            }
            '=' => {
                tokens.push(Token::Op("=".into()));
                i += 1;
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                tokens.push(Token::Op("!=".into()));
                i += 2;
            }
            '<' | '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Op(format!("{c}=")));
                    i += 2;
                } else {
                    tokens.push(Token::Op(c.to_string()));
                    i += 1;
                }
            }
            '\'' | '"' => {
                let end = chars[i + 1..].iter().position(|x| *x == c)? + i + 1;
                tokens.push(Token::Str(chars[i + 1..end].iter().collect()));
                i = end + 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                tokens.push(Token::Num(chars[start..i].iter().collect::<String>().parse().ok()?));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '-' | '.' | ':')) {
                    // `..` after a name is a separate step.
                    if chars[i] == '.' && chars.get(i + 1) == Some(&'.') {
                        break;
                    }
                    i += 1;
                }
                tokens.push(Token::Name(chars[start..i].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

#[derive(Debug, Clone)]
enum Axis {
    Child,
    DescendantOrSelf,
    Parent,
    Myself,
    Attribute,
    Descendant,
    Ancestor,
    AncestorOrSelf,
    FollowingSibling,
    PrecedingSibling,
}

#[derive(Debug, Clone)]
enum Test {
    Name(String),
    Any,
    Text,
    Node,
}

#[derive(Debug, Clone)]
struct Step {
    axis: Axis,
    test: Test,
    predicates: Vec<Expr>,
}

#[derive(Debug, Clone)]
enum Expr {
    /// A location path, absolute or relative to the context node.
    Path {
        absolute: bool,
        steps: Vec<Step>,
    },
    /// `(expr)[predicates]/steps`
    Filter {
        base: Box<Expr>,
        predicates: Vec<Expr>,
        steps: Vec<Step>,
    },
    Union(Vec<Expr>),
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Compare(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Str(String),
    Num(f64),
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        token
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Option<Expr> {
        let mut left = self.and()?;
        while matches!(self.peek(), Some(Token::Name(n)) if n == "or") {
            self.pos += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Some(left)
    }

    fn and(&mut self) -> Option<Expr> {
        let mut left = self.compare()?;
        while matches!(self.peek(), Some(Token::Name(n)) if n == "and") {
            self.pos += 1;
            left = Expr::And(Box::new(left), Box::new(self.compare()?));
        }
        Some(left)
    }

    fn compare(&mut self) -> Option<Expr> {
        let left = self.union()?;
        if let Some(Token::Op(op)) = self.peek().cloned() {
            self.pos += 1;
            let right = self.union()?;
            return Some(Expr::Compare(op, Box::new(left), Box::new(right)));
        }
        Some(left)
    }

    fn union(&mut self) -> Option<Expr> {
        let first = self.primary()?;
        if self.peek() != Some(&Token::Pipe) {
            return Some(first);
        }
        let mut items = vec![first];
        while self.eat(&Token::Pipe) {
            items.push(self.primary()?);
        }
        Some(Expr::Union(items))
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.peek()?.clone() {
            Token::Str(s) => {
                self.pos += 1;
                Some(Expr::Str(s))
            }
            Token::Num(n) => {
                self.pos += 1;
                Some(Expr::Num(n))
            }
            Token::LParen => {
                self.pos += 1;
                let inner = self.or()?;
                if !self.eat(&Token::RParen) {
                    return None;
                }
                let predicates = self.predicates()?;
                let steps = self.continuation()?;
                Some(Expr::Filter {
                    base: Box::new(inner),
                    predicates,
                    steps,
                })
            }
            Token::Name(name)
                if self.tokens.get(self.pos + 1) == Some(&Token::LParen)
                    && !matches!(name.as_str(), "text" | "node") =>
            {
                self.pos += 2;
                let mut args = Vec::new();
                if !self.eat(&Token::RParen) {
                    loop {
                        args.push(self.or()?);
                        if self.eat(&Token::Comma) {
                            continue;
                        }
                        if self.eat(&Token::RParen) {
                            break;
                        }
                        return None;
                    }
                }
                Some(Expr::Call(name, args))
            }
            _ => self.path(),
        }
    }

    fn predicates(&mut self) -> Option<Vec<Expr>> {
        let mut predicates = Vec::new();
        while self.eat(&Token::LBracket) {
            predicates.push(self.or()?);
            if !self.eat(&Token::RBracket) {
                return None;
            }
        }
        Some(predicates)
    }

    /// `/step` or `//step` parts after a filter expression.
    fn continuation(&mut self) -> Option<Vec<Step>> {
        let mut steps = Vec::new();
        loop {
            if self.eat(&Token::DoubleSlash) {
                steps.push(Step {
                    axis: Axis::DescendantOrSelf,
                    test: Test::Node,
                    predicates: Vec::new(),
                });
            } else if !self.eat(&Token::Slash) {
                break;
            }
            steps.push(self.step()?);
        }
        Some(steps)
    }

    fn path(&mut self) -> Option<Expr> {
        let mut steps = Vec::new();
        let absolute = match self.peek() {
            Some(Token::Slash) => {
                self.pos += 1;
                // `/` alone is the document.
                if !matches!(
                    self.peek(),
                    Some(Token::Name(_) | Token::Star | Token::At | Token::Dot | Token::DotDot)
                ) {
                    return Some(Expr::Path { absolute: true, steps });
                }
                true
            }
            Some(Token::DoubleSlash) => {
                self.pos += 1;
                steps.push(Step {
                    axis: Axis::DescendantOrSelf,
                    test: Test::Node,
                    predicates: Vec::new(),
                });
                true
            }
            _ => false,
        };
        steps.push(self.step()?);
        steps.extend(self.continuation()?);
        Some(Expr::Path { absolute, steps })
    }

    fn step(&mut self) -> Option<Step> {
        let (axis, test) = match self.next()? {
            Token::Dot => (Axis::Myself, Test::Node),
            Token::DotDot => (Axis::Parent, Test::Node),
            Token::At => match self.next()? {
                Token::Name(n) => (Axis::Attribute, Test::Name(n)),
                Token::Star => (Axis::Attribute, Test::Any),
                _ => return None,
            },
            Token::Star => (Axis::Child, Test::Any),
            Token::Name(n) if n == "text" || n == "node" => {
                if !(self.eat(&Token::LParen) && self.eat(&Token::RParen)) {
                    return None;
                }
                (Axis::Child, if n == "text" { Test::Text } else { Test::Node })
            }
            Token::Name(n) if n.contains("::") => {
                let (axis, test) = n.split_once("::")?;
                let axis = match axis {
                    "child" => Axis::Child,
                    "descendant" => Axis::Descendant,
                    "descendant-or-self" => Axis::DescendantOrSelf,
                    "parent" => Axis::Parent,
                    "self" => Axis::Myself,
                    "attribute" => Axis::Attribute,
                    "ancestor" => Axis::Ancestor,
                    "ancestor-or-self" => Axis::AncestorOrSelf,
                    "following-sibling" => Axis::FollowingSibling,
                    "preceding-sibling" => Axis::PrecedingSibling,
                    // `following::`, `preceding::` and namespaces are outside the subset.
                    _ => return None,
                };
                let test = match test {
                    "" if self.eat(&Token::Star) => Test::Any,
                    "" => return None,
                    "text" | "node" if self.eat(&Token::LParen) => {
                        if !self.eat(&Token::RParen) {
                            return None;
                        }
                        if test == "text" {
                            Test::Text
                        } else {
                            Test::Node
                        }
                    }
                    name => Test::Name(name.to_string()),
                };
                (axis, test)
            }
            Token::Name(n) => (Axis::Child, Test::Name(n)),
            _ => return None,
        };
        let predicates = self.predicates()?;
        Some(Step { axis, test, predicates })
    }
}

fn parse(expr: &str) -> Option<Expr> {
    let mut parser = Parser {
        tokens: tokenize(expr)?,
        pos: 0,
    };
    let parsed = parser.or()?;
    (parser.pos == parser.tokens.len()).then_some(parsed)
}

/// A node of a node-set: an element or text node, or an attribute of one.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Node(NodeId),
    Attr(NodeId, String),
}

#[derive(Debug, Clone)]
enum Value {
    Nodes(Vec<Item>),
    Str(String),
    Num(f64),
    Bool(bool),
}

struct Eval<'a> {
    tree: &'a Tree,
    order: Vec<usize>,
}

impl Eval<'_> {
    fn item_string(&self, item: &Item) -> String {
        match item {
            Item::Node(node) => self.tree.string_value(*node),
            Item::Attr(node, name) => self.tree.attr(*node, name).unwrap_or_default().to_string(),
        }
    }

    fn string(&self, value: &Value) -> String {
        match value {
            Value::Nodes(items) => items.first().map(|i| self.item_string(i)).unwrap_or_default(),
            Value::Str(s) => s.clone(),
            Value::Num(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Value::Bool(b) => b.to_string(),
        }
    }

    fn number(&self, value: &Value) -> f64 {
        match value {
            Value::Num(n) => *n,
            Value::Bool(b) => f64::from(u8::from(*b)),
            other => self.string(other).trim().parse().unwrap_or(f64::NAN),
        }
    }

    fn boolean(&self, value: &Value) -> bool {
        match value {
            Value::Nodes(items) => !items.is_empty(),
            Value::Str(s) => !s.is_empty(),
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Bool(b) => *b,
        }
    }

    fn sort(&self, mut items: Vec<Item>) -> Vec<Item> {
        let key = |item: &Item| match item {
            Item::Node(n) => (self.order[*n], String::new()),
            Item::Attr(n, a) => (self.order[*n], a.clone()),
        };
        items.sort_by_key(key);
        items.dedup();
        items
    }

    fn step(&self, context: &[Item], step: &Step) -> Option<Vec<Item>> {
        let mut out = Vec::new();
        for item in context {
            let Item::Node(node) = item else { continue };
            let node = *node;
            let candidates: Vec<Item> = match step.axis {
                Axis::Child => self.tree.children(node).iter().map(|c| Item::Node(*c)).collect(),
                Axis::DescendantOrSelf => std::iter::once(node)
                    .chain(self.tree.descendants(node))
                    .map(Item::Node)
                    .collect(),
                Axis::Parent => self.tree.parent(node).map(Item::Node).into_iter().collect(),
                Axis::Myself => vec![Item::Node(node)],
                Axis::Descendant => self.tree.descendants(node).into_iter().map(Item::Node).collect(),
                // Reverse axes: nearest first, as their positions count.
                Axis::Ancestor | Axis::AncestorOrSelf => {
                    let mut chain = Vec::new();
                    let mut current = match step.axis {
                        Axis::Ancestor => self.tree.parent(node),
                        _ => Some(node),
                    };
                    while let Some(n) = current {
                        chain.push(Item::Node(n));
                        current = self.tree.parent(n);
                    }
                    chain
                }
                Axis::FollowingSibling | Axis::PrecedingSibling => {
                    let siblings = self.tree.parent(node).map(|p| self.tree.children(p)).unwrap_or(&[]);
                    let at = siblings.iter().position(|c| *c == node).unwrap_or(0);
                    if matches!(step.axis, Axis::FollowingSibling) {
                        siblings[at + 1..].iter().map(|c| Item::Node(*c)).collect()
                    } else {
                        siblings[..at].iter().rev().map(|c| Item::Node(*c)).collect()
                    }
                }
                Axis::Attribute => self
                    .tree
                    .attrs(node)
                    .iter()
                    .filter(|(n, _)| matches!(&step.test, Test::Any) || matches!(&step.test, Test::Name(t) if t == n))
                    .map(|(n, _)| Item::Attr(node, n.clone()))
                    .collect(),
            };
            let matched: Vec<Item> = candidates
                .into_iter()
                .filter(|candidate| match (candidate, &step.axis) {
                    (Item::Attr(..), _) => true,
                    (Item::Node(n), Axis::Parent | Axis::Myself | Axis::DescendantOrSelf)
                        if matches!(step.test, Test::Node) =>
                    {
                        let _ = n;
                        true
                    }
                    (Item::Node(n), _) => match &step.test {
                        Test::Name(name) => self.tree.tag(*n) == name,
                        Test::Any => self.tree.is_element(*n),
                        Test::Text => !self.tree.is_element(*n) && *n != 0,
                        Test::Node => true,
                    },
                })
                .collect();
            out.extend(self.filter(matched, &step.predicates)?);
        }
        Some(out)
    }

    /// Applies predicates to a node-set, each with positions in it.
    fn filter(&self, mut items: Vec<Item>, predicates: &[Expr]) -> Option<Vec<Item>> {
        for predicate in predicates {
            let size = items.len();
            let mut kept = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let value = self.eval(predicate, item, i + 1, size)?;
                let keep = match value {
                    Value::Num(n) => n == (i + 1) as f64,
                    other => self.boolean(&other),
                };
                if keep {
                    kept.push(item.clone());
                }
            }
            items = kept;
        }
        Some(items)
    }

    fn path(&self, start: Vec<Item>, steps: &[Step]) -> Option<Vec<Item>> {
        let mut current = start;
        for step in steps {
            current = self.sort(self.step(&current, step)?);
        }
        Some(current)
    }

    fn eval(&self, expr: &Expr, context: &Item, position: usize, size: usize) -> Option<Value> {
        Some(match expr {
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Num(n) => Value::Num(*n),
            Expr::Path { absolute, steps } => {
                let start = if *absolute {
                    vec![Item::Node(0)]
                } else {
                    vec![context.clone()]
                };
                Value::Nodes(self.path(start, steps)?)
            }
            Expr::Filter {
                base,
                predicates,
                steps,
            } => {
                let Value::Nodes(items) = self.eval(base, context, position, size)? else {
                    return None;
                };
                let items = self.filter(self.sort(items), predicates)?;
                Value::Nodes(self.path(items, steps)?)
            }
            Expr::Union(parts) => {
                let mut items = Vec::new();
                for part in parts {
                    let Value::Nodes(found) = self.eval(part, context, position, size)? else {
                        return None;
                    };
                    items.extend(found);
                }
                Value::Nodes(self.sort(items))
            }
            Expr::Or(a, b) => Value::Bool(
                self.boolean(&self.eval(a, context, position, size)?)
                    || self.boolean(&self.eval(b, context, position, size)?),
            ),
            Expr::And(a, b) => Value::Bool(
                self.boolean(&self.eval(a, context, position, size)?)
                    && self.boolean(&self.eval(b, context, position, size)?),
            ),
            Expr::Compare(op, a, b) => {
                let left = self.eval(a, context, position, size)?;
                let right = self.eval(b, context, position, size)?;
                Value::Bool(self.compare(op, &left, &right))
            }
            Expr::Call(name, args) => {
                let values: Option<Vec<Value>> = args.iter().map(|a| self.eval(a, context, position, size)).collect();
                let values = values?;
                let context_string = || self.item_string(context);
                let arg = |i: usize| values.get(i).map(|v| self.string(v));
                match name.as_str() {
                    "contains" => Value::Bool(arg(0)?.contains(&arg(1)?)),
                    "starts-with" => Value::Bool(arg(0)?.starts_with(&arg(1)?)),
                    "not" => Value::Bool(!self.boolean(values.first()?)),
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "position" => Value::Num(position as f64),
                    "last" => Value::Num(size as f64),
                    "count" => match values.first()? {
                        Value::Nodes(items) => Value::Num(items.len() as f64),
                        _ => return None,
                    },
                    "string" => Value::Str(arg(0).unwrap_or_else(context_string)),
                    "normalize-space" => Value::Str(
                        arg(0)
                            .unwrap_or_else(context_string)
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    "local-name" | "name" => {
                        let item = match values.first() {
                            Some(Value::Nodes(items)) => items.first().cloned(),
                            Some(_) => return None,
                            None => Some(context.clone()),
                        };
                        Value::Str(match item {
                            Some(Item::Node(n)) => self.tree.tag(n).to_string(),
                            Some(Item::Attr(_, a)) => a,
                            None => String::new(),
                        })
                    }
                    // Odoo's: every class given is in the node's `class`.
                    "hasclass" => {
                        let Item::Node(node) = context else {
                            return Some(Value::Bool(false));
                        };
                        let classes: Vec<&str> = self
                            .tree
                            .attr(*node, "class")
                            .unwrap_or_default()
                            .split_whitespace()
                            .collect();
                        Value::Bool(values.iter().all(|v| classes.contains(&self.string(v).as_str())))
                    }
                    _ => return None,
                }
            }
        })
    }

    fn compare(&self, op: &str, left: &Value, right: &Value) -> bool {
        let strings = |value: &Value| -> Vec<String> {
            match value {
                Value::Nodes(items) => items.iter().map(|i| self.item_string(i)).collect(),
                other => vec![self.string(other)],
            }
        };
        match op {
            "=" | "!=" => {
                let numeric = matches!(left, Value::Num(_)) || matches!(right, Value::Num(_));
                let (ls, rs) = (strings(left), strings(right));
                ls.iter().any(|l| {
                    rs.iter().any(|r| {
                        let equal = if numeric {
                            l.trim().parse::<f64>().ok() == r.trim().parse::<f64>().ok()
                        } else {
                            l == r
                        };
                        if op == "=" {
                            equal
                        } else {
                            !equal
                        }
                    })
                })
            }
            _ => {
                let (l, r) = (self.number(left), self.number(right));
                match op {
                    "<" => l < r,
                    "<=" => l <= r,
                    ">" => l > r,
                    _ => l >= r,
                }
            }
        }
    }
}

impl Tree {
    /// The elements `expr` selects, evaluated with the root element as the
    /// context node, in document order. `None` when `expr` is outside the
    /// supported subset.
    pub fn select(&self, expr: &str) -> Option<Vec<NodeId>> {
        let parsed = parse(expr)?;
        let root = self.root()?;
        let eval = Eval {
            tree: self,
            order: self.order(),
        };
        match eval.eval(&parsed, &Item::Node(root), 1, 1)? {
            Value::Nodes(items) => Some(
                items
                    .into_iter()
                    .filter_map(|i| match i {
                        Item::Node(n) => Some(n),
                        Item::Attr(..) => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}

// --- Inheritance ------------------------------------------------------------

/// Why a spec could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// The spec matches nothing in the parent view.
    NotFound,
    /// The spec uses XPath outside the supported subset: not checked.
    Unsupported,
}

/// The node a spec (from `spec_tree`) targets in `tree`, as Odoo's
/// `locate_node` finds it.
fn locate(tree: &Tree, spec_tree: &Tree, spec: NodeId) -> Result<NodeId, SpecError> {
    let tag = spec_tree.tag(spec);
    if tag == "xpath" {
        let expr = spec_tree.attr(spec, "expr").ok_or(SpecError::Unsupported)?;
        let found = tree.select(expr).ok_or(SpecError::Unsupported)?;
        return found
            .into_iter()
            .find(|n| tree.is_element(*n))
            .ok_or(SpecError::NotFound);
    }
    let root = tree.root().ok_or(SpecError::NotFound)?;
    let candidates = std::iter::once(root).chain(tree.descendants(root));
    if tag == "field" {
        let name = spec_tree.attr(spec, "name").ok_or(SpecError::Unsupported)?;
        return candidates
            .into_iter()
            .find(|n| tree.tag(*n) == "field" && tree.attr(*n, "name") == Some(name))
            .ok_or(SpecError::NotFound);
    }
    let wanted: Vec<&(String, String)> = spec_tree
        .attrs(spec)
        .iter()
        .filter(|(n, _)| !matches!(n.as_str(), "position" | "version"))
        .collect();
    candidates
        .into_iter()
        .find(|n| tree.tag(*n) == tag && wanted.iter().all(|(k, v)| tree.attr(*n, k) == Some(v.as_str())))
        .ok_or(SpecError::NotFound)
}

/// Copies the content of a spec (children of `spec` in `spec_tree`) into
/// `tree`, detached; `move` children are taken from `tree` instead.
fn content(tree: &mut Tree, spec_tree: &Tree, spec: NodeId) -> Result<Vec<NodeId>, SpecError> {
    let mut out = Vec::new();
    for child in spec_tree.children(spec) {
        if spec_tree.is_element(*child) && spec_tree.attr(*child, "position") == Some("move") {
            out.push(locate(tree, spec_tree, *child)?);
            continue;
        }
        if !spec_tree.is_element(*child) && spec_tree.string_value(*child).trim().is_empty() {
            continue;
        }
        let copy = copy_between(spec_tree, *child, tree);
        out.push(copy);
    }
    Ok(out)
}

fn copy_between(from: &Tree, node: NodeId, to: &mut Tree) -> NodeId {
    let id = to.nodes.len();
    to.nodes.push(NodeData {
        kind: from.nodes[node].kind.clone(),
        parent: None,
        children: Vec::new(),
    });
    for child in from.children(node).to_vec() {
        let copy = copy_between(from, child, to);
        to.append(id, copy);
    }
    id
}

/// Applies one spec to `tree`, as Odoo's `apply_inheritance_specs` does.
pub fn apply_spec(tree: &mut Tree, spec_tree: &Tree, spec: NodeId) -> Result<(), SpecError> {
    if spec_tree.tag(spec) == "data" {
        for child in spec_tree.element_children(spec) {
            apply_spec(tree, spec_tree, child)?;
        }
        return Ok(());
    }
    let node = locate(tree, spec_tree, spec)?;
    let position = spec_tree.attr(spec, "position").unwrap_or("inside");
    match position {
        "attributes" => {
            for attribute in spec_tree.element_children(spec) {
                if spec_tree.tag(attribute) != "attribute" {
                    continue;
                }
                let Some(name) = spec_tree.attr(attribute, "name") else {
                    continue;
                };
                let add = spec_tree.attr(attribute, "add");
                let remove = spec_tree.attr(attribute, "remove");
                if add.is_some() || remove.is_some() {
                    let separator = spec_tree.attr(attribute, "separator").unwrap_or(",");
                    let current = tree.attr(node, name).unwrap_or_default().to_string();
                    let mut values: Vec<String> = current
                        .split(separator)
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect();
                    if let Some(remove) = remove {
                        let gone: Vec<&str> = remove.split(separator).map(str::trim).collect();
                        values.retain(|v| !gone.contains(&v.as_str()));
                    }
                    if let Some(add) = add {
                        values.extend(
                            add.split(separator)
                                .map(|v| v.trim().to_string())
                                .filter(|v| !v.is_empty()),
                        );
                    }
                    let joined = values.join(if separator == " " { " " } else { separator });
                    tree.set_attr(node, name, (!joined.is_empty()).then_some(joined));
                } else {
                    let value = spec_tree.string_value(attribute);
                    tree.set_attr(node, name, (!value.is_empty()).then_some(value));
                }
            }
        }
        "replace" => {
            let parent = tree.parent(node).ok_or(SpecError::NotFound)?;
            if spec_tree.attr(spec, "mode") == Some("inner") {
                let new = content(tree, spec_tree, spec)?;
                for child in tree.children(node).to_vec() {
                    tree.detach(child);
                }
                for child in new {
                    tree.append(node, child);
                }
            } else {
                let index = tree.children(parent).iter().position(|c| *c == node).unwrap_or(0);
                let new = content(tree, spec_tree, spec)?;
                tree.detach(node);
                // `$0` in the new content stands for the replaced node.
                let placeholder = new
                    .iter()
                    .flat_map(|n| std::iter::once(*n).chain(tree.descendants(*n)))
                    .find(|n| !tree.is_element(*n) && tree.string_value(*n).trim() == "$0");
                if let Some(placeholder) = placeholder {
                    if let Some(holder) = tree.parent(placeholder) {
                        let at = tree
                            .children(holder)
                            .iter()
                            .position(|c| *c == placeholder)
                            .unwrap_or(0);
                        tree.detach(placeholder);
                        tree.insert_at(holder, at, node);
                    }
                }
                for (i, child) in new.into_iter().enumerate() {
                    if child == placeholder.unwrap_or(usize::MAX) {
                        tree.insert_at(parent, index + i, node);
                        continue;
                    }
                    tree.insert_at(parent, index + i, child);
                }
            }
        }
        "inside" => {
            for child in content(tree, spec_tree, spec)? {
                tree.append(node, child);
            }
        }
        "after" | "before" => {
            let parent = tree.parent(node).ok_or(SpecError::NotFound)?;
            let new = content(tree, spec_tree, spec)?;
            let index = tree.children(parent).iter().position(|c| *c == node).unwrap_or(0);
            let start = if position == "after" { index + 1 } else { index };
            for (i, child) in new.into_iter().enumerate() {
                tree.insert_at(parent, start + i, child);
            }
        }
        _ => return Err(SpecError::Unsupported),
    }
    Ok(())
}

/// The specs of an extension arch: its root, or the children of a `<data>`
/// root (nested `<data>` included).
pub fn specs(tree: &Tree) -> Vec<NodeId> {
    let Some(root) = tree.root() else { return Vec::new() };
    if tree.tag(root) == "data" {
        let mut out = Vec::new();
        let mut stack = tree.element_children(root);
        stack.reverse();
        while let Some(node) = stack.pop() {
            if tree.tag(node) == "data" {
                let mut nested = tree.element_children(node);
                nested.reverse();
                stack.extend(nested);
            } else {
                out.push(node);
            }
        }
        out
    } else {
        vec![root]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(xml: &str) -> Tree {
        let doc = roxmltree::Document::parse(xml).unwrap();
        Tree::from_node(doc.root_element())
    }

    const FORM: &str = r#"<form><header><button name="confirm" class="btn btn-primary"/></header>
        <sheet><group name="main"><field name="partner_id"/><field name="date"/></group>
        <notebook><page name="lines"><field name="order_line"><list><field name="product_id"/><field name="qty"/></list></field></page></notebook>
        <div class="oe_chatter"><img src="x.png"/></div></sheet></form>"#;

    fn tags(t: &Tree, expr: &str) -> Vec<String> {
        t.select(expr)
            .unwrap_or_else(|| panic!("{expr} unsupported"))
            .into_iter()
            .map(|n| match t.attr(n, "name") {
                Some(name) => format!("{}:{name}", t.tag(n)),
                None => t.tag(n).to_string(),
            })
            .collect()
    }

    #[test]
    fn xpaths() {
        let t = tree(FORM);
        assert_eq!(tags(&t, "//field[@name='qty']"), ["field:qty"]);
        assert_eq!(
            tags(&t, "//field[@name='order_line']/list/field[@name='product_id']"),
            ["field:product_id"]
        );
        assert_eq!(tags(&t, "//div[hasclass('oe_chatter')]"), ["div"]);
        assert_eq!(tags(&t, "//button[hasclass('btn', 'btn-primary')]"), ["button:confirm"]);
        assert_eq!(tags(&t, "//button[hasclass('btn', 'missing')]"), Vec::<String>::new());
        assert_eq!(tags(&t, "."), ["form"]);
        assert_eq!(tags(&t, "/form/sheet/group"), ["group:main"]);
        assert_eq!(tags(&t, "sheet/group/field[2]"), ["field:date"]);
        assert_eq!(tags(&t, "(//field)[1]"), ["field:partner_id"]);
        assert_eq!(tags(&t, "//img/../.."), ["sheet"]);
        assert_eq!(tags(&t, "//*[local-name()='page']"), ["page:lines"]);
        assert_eq!(
            tags(&t, "//field[@name='qty' or @name='date']"),
            ["field:date", "field:qty"]
        );
        // Positions count among the children of each parent, as in lxml.
        assert_eq!(
            tags(&t, "//field[not(@name='qty')][last()]"),
            ["field:date", "field:order_line", "field:product_id"]
        );
        assert_eq!(
            tags(&t, "//group[@name='main']//field[contains(@name, 'part')]"),
            ["field:partner_id"]
        );
        assert_eq!(tags(&t, "//page | //header"), ["header", "page:lines"]);
        assert_eq!(
            tags(&t, "//field[@name='partner_id']/following-sibling::field[1]"),
            ["field:date"]
        );
        // Reverse axes count positions from the nearest node.
        assert_eq!(
            tags(&t, "//field[@name='date']/preceding-sibling::*[1]"),
            ["field:partner_id"]
        );
        assert_eq!(tags(&t, "//field[@name='partner_id']/ancestor::*[1]"), ["group:main"]);
        assert_eq!(tags(&t, "//img/ancestor-or-self::sheet"), ["sheet"]);
        assert!(
            t.select("//field[@name='x']/following::group").is_none(),
            "outside the subset"
        );
    }

    #[test]
    fn inheritance() {
        let mut t = tree(FORM);
        let spec = tree(
            r#"<data>
                <field name="date" position="after"><field name="note"/></field>
                <xpath expr="//field[@name='note']" position="before"><field name="ref"/></xpath>
                <xpath expr="//header/button" position="attributes"><attribute name="class" add="o_x" separator=" "/><attribute name="string">Go</attribute></xpath>
                <div class="oe_chatter" position="replace"/>
                <page name="lines" position="inside"><xpath expr="//field[@name='qty']" position="move"/></page>
            </data>"#,
        );
        for s in specs(&spec) {
            apply_spec(&mut t, &spec, s).unwrap();
        }
        assert_eq!(
            tags(&t, "//group/field"),
            ["field:partner_id", "field:date", "field:ref", "field:note"]
        );
        let button = t.select("//button").unwrap()[0];
        assert_eq!(t.attr(button, "class"), Some("btn btn-primary o_x"));
        assert_eq!(t.attr(button, "string"), Some("Go"));
        assert!(t.select("//div[hasclass('oe_chatter')]").unwrap().is_empty());
        assert_eq!(tags(&t, "//page/field"), ["field:order_line", "field:qty"]);
        // `$0`: the replaced node, wrapped.
        let wrap = tree(r#"<xpath expr="//notebook" position="replace"><div class="w">$0</div></xpath>"#);
        apply_spec(&mut t, &wrap, wrap.root().unwrap()).unwrap();
        assert_eq!(tags(&t, "//div[hasclass('w')]/notebook/page"), ["page:lines"]);
        // A spec that matches nothing.
        let missing = tree(r#"<xpath expr="//field[@name='nope']" position="after"><field name="x"/></xpath>"#);
        assert_eq!(
            apply_spec(&mut t, &missing, missing.root().unwrap()),
            Err(SpecError::NotFound)
        );
    }
}
