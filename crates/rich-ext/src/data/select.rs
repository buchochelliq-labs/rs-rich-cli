//! Pluggable selection expressions.
//!
//! A [`SelectorBackend`] compiles an expression into a [`Selector`], which
//! picks nodes out of a document. [`Selectors`] is a small registry of
//! backends by name, so a tool can offer `--select jsonpath:…` today and
//! other languages later. With the `jsonpath` feature the registry starts
//! with the built-in `JsonPath` backend.

use std::fmt;

use super::{Node, Path};

/// A compile or evaluation failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectError {
    pub message: String,
    /// The 1-based character column of the offending character.
    pub column: Option<usize>,
}

impl SelectError {
    pub fn new(message: impl Into<String>, column: Option<usize>) -> Self {
        SelectError {
            message: message.into(),
            column,
        }
    }
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.column {
            Some(column) => write!(f, "{} at column {column}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for SelectError {}

/// A compiled expression.
pub trait Selector {
    /// The selected nodes with their paths, in the expression's order.
    fn select<'a>(&self, root: &'a Node) -> Result<Vec<(Path, &'a Node)>, SelectError>;
}

/// An expression language.
pub trait SelectorBackend {
    /// The name it registers under (`jsonpath`).
    fn name(&self) -> &str;
    /// Compile `expr`.
    fn compile(&self, expr: &str) -> Result<Box<dyn Selector>, SelectError>;
}

/// Backends by name.
///
/// ```
/// use rich_ext::data::{parse, Format, Selectors};
///
/// let node = parse(Format::Json, r#"{"servers": [{"port": 80}, {"port": 443}]}"#).unwrap();
/// # #[cfg(feature = "jsonpath")] {
/// let selector = Selectors::default().compile("jsonpath", "$.servers[*].port").unwrap();
/// let ports: Vec<String> = selector.select(&node).unwrap().iter().map(|(p, _)| p.to_string()).collect();
/// assert_eq!(ports, ["servers[0].port", "servers[1].port"]);
/// # }
/// ```
pub struct Selectors {
    backends: Vec<Box<dyn SelectorBackend>>,
}

impl Default for Selectors {
    /// The built-in backends: `JsonPath` with the `jsonpath` feature,
    /// none otherwise.
    fn default() -> Self {
        #[allow(unused_mut)]
        let mut selectors = Selectors::new();
        #[cfg(feature = "jsonpath")]
        selectors.register(Box::new(JsonPath));
        selectors
    }
}

impl Selectors {
    /// An empty registry.
    pub fn new() -> Self {
        Selectors {
            backends: Vec::new(),
        }
    }

    /// Add a backend, replacing any with the same name.
    pub fn register(&mut self, backend: Box<dyn SelectorBackend>) -> &mut Self {
        self.backends.retain(|b| b.name() != backend.name());
        self.backends.push(backend);
        self
    }

    /// The backend called `name`.
    pub fn get(&self, name: &str) -> Option<&dyn SelectorBackend> {
        self.backends
            .iter()
            .find(|b| b.name() == name)
            .map(|b| &**b)
    }

    /// Registered names, in registration order.
    pub fn names(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.name()).collect()
    }

    /// Compile `expr` with the backend called `backend`.
    pub fn compile(&self, backend: &str, expr: &str) -> Result<Box<dyn Selector>, SelectError> {
        match self.get(backend) {
            Some(b) => b.compile(expr),
            None => Err(SelectError::new(
                format!("no selector backend named `{backend}`"),
                None,
            )),
        }
    }
}

#[cfg(feature = "jsonpath")]
pub use jsonpath::{JsonPath, JsonPathSelector};

#[cfg(feature = "jsonpath")]
mod jsonpath {
    use std::cmp::Ordering;

    use super::{SelectError, Selector, SelectorBackend};
    use crate::data::{value_eq, Node, Path, PathSegment, Value};

    /// The built-in JSONPath backend (`jsonpath`).
    ///
    /// Supported: `$`, `.key`, `['key']` / `["key"]`, `[n]`, `[-n]`, `[*]`,
    /// `.*`, `..key` / `..*` / `..[…]` (recursive descent), slices
    /// `[a:b]` / `[a:b:step]`, unions `[0,2]` / `['a','b']`, and filters
    /// `[?(@.k == v)]`, `[?(@.k)]` (existence), `[?@.k > 1]` with `==`,
    /// `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!` and parentheses, comparing
    /// numbers, strings, booleans and `null`. Filter paths may start at `@`
    /// (the candidate) or `$` (the root). The leading `$` may be omitted:
    /// `servers[0].name` means `$.servers[0].name`.
    ///
    /// ```
    /// use rich_ext::data::{parse, Format, JsonPathSelector, Selector};
    ///
    /// let node = parse(Format::Json, r#"{"items": [{"n": 1}, {"n": 5}, {"n": 9}]}"#).unwrap();
    /// let selector = JsonPathSelector::parse("$.items[?(@.n > 2)].n").unwrap();
    /// let found: Vec<_> = selector.select(&node).unwrap().into_iter().map(|(p, _)| p.to_string()).collect();
    /// assert_eq!(found, ["items[1].n", "items[2].n"]);
    ///
    /// let error = JsonPathSelector::parse("$.items[1").unwrap_err();
    /// assert_eq!(error.to_string(), "expected `]` at column 10");
    /// ```
    #[derive(Clone, Copy, Debug, Default)]
    pub struct JsonPath;

    impl SelectorBackend for JsonPath {
        fn name(&self) -> &str {
            "jsonpath"
        }
        fn compile(&self, expr: &str) -> Result<Box<dyn Selector>, SelectError> {
            Ok(Box::new(JsonPathSelector::parse(expr)?))
        }
    }

    /// A compiled JSONPath expression.
    #[derive(Clone, Debug)]
    pub struct JsonPathSelector {
        steps: Vec<Step>,
    }

    #[derive(Clone, Debug)]
    struct Step {
        descendant: bool,
        selectors: Vec<Sel>,
    }

    #[derive(Clone, Debug)]
    enum Sel {
        Name(String),
        Wildcard,
        Index(i64),
        Slice(Option<i64>, Option<i64>, i64),
        Filter(Box<Expr>),
    }

    #[derive(Clone, Debug)]
    enum Expr {
        Or(Box<Expr>, Box<Expr>),
        And(Box<Expr>, Box<Expr>),
        Not(Box<Expr>),
        Compare(Operand, Op, Operand),
        Exists(Operand),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Op {
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
    }

    #[derive(Clone, Debug)]
    enum Operand {
        Current(Vec<PathSegment>, Vec<i64>),
        Root(Vec<PathSegment>, Vec<i64>),
        Literal(Node),
    }

    struct Parser {
        chars: Vec<char>,
        pos: usize,
    }

    fn is_name_char(c: char) -> bool {
        !c.is_whitespace() && !".[]()?,=!<>&|'\"*$:".contains(c)
    }

    impl Parser {
        fn error<T>(&self, message: impl Into<String>) -> Result<T, SelectError> {
            Err(SelectError::new(message, Some(self.pos + 1)))
        }
        fn peek(&self) -> Option<char> {
            self.chars.get(self.pos).copied()
        }
        fn peek_at(&self, offset: usize) -> Option<char> {
            self.chars.get(self.pos + offset).copied()
        }
        fn eat(&mut self, c: char) -> bool {
            if self.peek() == Some(c) {
                self.pos += 1;
                true
            } else {
                false
            }
        }
        fn expect(&mut self, c: char) -> Result<(), SelectError> {
            if self.eat(c) {
                Ok(())
            } else {
                self.error(format!("expected `{c}`"))
            }
        }
        fn blanks(&mut self) {
            while self.peek().is_some_and(char::is_whitespace) {
                self.pos += 1;
            }
        }

        fn name(&mut self) -> Result<String, SelectError> {
            let start = self.pos;
            while self.peek().is_some_and(is_name_char) {
                self.pos += 1;
            }
            if start == self.pos {
                return match self.peek() {
                    Some(c) => self.error(format!("unexpected `{c}`, expected a key")),
                    None => self.error("expected a key"),
                };
            }
            Ok(self.chars[start..self.pos].iter().collect())
        }

        fn string(&mut self) -> Result<String, SelectError> {
            let quote = self.peek().expect("called at a quote");
            let open = self.pos;
            self.pos += 1;
            let mut out = String::new();
            loop {
                match self.peek() {
                    None => {
                        self.pos = open;
                        return self.error("unterminated string");
                    }
                    Some(c) if c == quote => {
                        self.pos += 1;
                        return Ok(out);
                    }
                    Some('\\') => {
                        self.pos += 1;
                        let escaped = match self.peek() {
                            Some('n') => '\n',
                            Some('t') => '\t',
                            Some('r') => '\r',
                            Some(c @ ('\\' | '\'' | '"' | '/')) => c,
                            Some(c) => return self.error(format!("unknown escape `\\{c}`")),
                            None => return self.error("unterminated string"),
                        };
                        out.push(escaped);
                        self.pos += 1;
                    }
                    Some(c) => {
                        out.push(c);
                        self.pos += 1;
                    }
                }
            }
        }

        fn integer(&mut self) -> Result<i64, SelectError> {
            let start = self.pos;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
            let text: String = self.chars[start..self.pos].iter().collect();
            text.parse().or_else(|_| {
                self.pos = start;
                self.error("expected an integer")
            })
        }

        fn path(&mut self) -> Result<Vec<Step>, SelectError> {
            let mut steps = Vec::new();
            self.blanks();
            if !self.eat('$') && self.peek().is_some_and(is_name_char) {
                steps.push(Step {
                    descendant: false,
                    selectors: vec![Sel::Name(self.name()?)],
                });
            }
            loop {
                match self.peek() {
                    None => break,
                    Some('.') if self.peek_at(1) == Some('.') => {
                        self.pos += 2;
                        let selectors = match self.peek() {
                            Some('[') => self.bracket()?,
                            Some('*') => {
                                self.pos += 1;
                                vec![Sel::Wildcard]
                            }
                            _ => vec![Sel::Name(self.name()?)],
                        };
                        steps.push(Step {
                            descendant: true,
                            selectors,
                        });
                    }
                    Some('.') => {
                        self.pos += 1;
                        let selectors = if self.eat('*') {
                            vec![Sel::Wildcard]
                        } else {
                            vec![Sel::Name(self.name()?)]
                        };
                        steps.push(Step {
                            descendant: false,
                            selectors,
                        });
                    }
                    Some('[') => {
                        let selectors = self.bracket()?;
                        steps.push(Step {
                            descendant: false,
                            selectors,
                        });
                    }
                    Some(c) if c.is_whitespace() => {
                        self.blanks();
                        if self.peek().is_some() {
                            return self.error("unexpected text after the path");
                        }
                    }
                    Some(c) => return self.error(format!("unexpected `{c}`")),
                }
            }
            Ok(steps)
        }

        /// `[ … ]`: a wildcard, a filter, or a union of names, indexes and
        /// slices.
        fn bracket(&mut self) -> Result<Vec<Sel>, SelectError> {
            self.expect('[')?;
            self.blanks();
            let mut selectors = Vec::new();
            loop {
                self.blanks();
                match self.peek() {
                    Some('*') => {
                        self.pos += 1;
                        selectors.push(Sel::Wildcard);
                    }
                    Some('?') => {
                        self.pos += 1;
                        self.blanks();
                        selectors.push(Sel::Filter(Box::new(self.or()?)));
                    }
                    Some('\'' | '"') => selectors.push(Sel::Name(self.string()?)),
                    Some(c) if c == '-' || c == ':' || c.is_ascii_digit() => {
                        selectors.push(self.index_or_slice()?);
                    }
                    Some(c) => return self.error(format!("unexpected `{c}` in brackets")),
                    None => return self.error("expected `]`"),
                }
                self.blanks();
                if !self.eat(',') {
                    break;
                }
            }
            self.expect(']')?;
            Ok(selectors)
        }

        fn index_or_slice(&mut self) -> Result<Sel, SelectError> {
            let bound = |p: &mut Parser| -> Result<Option<i64>, SelectError> {
                p.blanks();
                if p.peek().is_some_and(|c| c == '-' || c.is_ascii_digit()) {
                    p.integer().map(Some)
                } else {
                    Ok(None)
                }
            };
            let start = bound(self)?;
            self.blanks();
            if !self.eat(':') {
                return match start {
                    Some(index) => Ok(Sel::Index(index)),
                    None => self.error("expected an index"),
                };
            }
            let end = bound(self)?;
            self.blanks();
            let step = if self.eat(':') {
                let at = self.pos;
                match bound(self)? {
                    Some(0) => {
                        self.pos = at;
                        return self.error("slice step cannot be 0");
                    }
                    Some(step) => step,
                    None => 1,
                }
            } else {
                1
            };
            Ok(Sel::Slice(start, end, step))
        }

        fn or(&mut self) -> Result<Expr, SelectError> {
            let mut left = self.and()?;
            loop {
                self.blanks();
                if self.peek() == Some('|') && self.peek_at(1) == Some('|') {
                    self.pos += 2;
                    left = Expr::Or(Box::new(left), Box::new(self.and()?));
                } else {
                    return Ok(left);
                }
            }
        }

        fn and(&mut self) -> Result<Expr, SelectError> {
            let mut left = self.unary()?;
            loop {
                self.blanks();
                if self.peek() == Some('&') && self.peek_at(1) == Some('&') {
                    self.pos += 2;
                    left = Expr::And(Box::new(left), Box::new(self.unary()?));
                } else {
                    return Ok(left);
                }
            }
        }

        fn unary(&mut self) -> Result<Expr, SelectError> {
            self.blanks();
            if self.peek() == Some('!') && self.peek_at(1) != Some('=') {
                self.pos += 1;
                return Ok(Expr::Not(Box::new(self.unary()?)));
            }
            if self.eat('(') {
                let inner = self.or()?;
                self.blanks();
                self.expect(')')?;
                return Ok(inner);
            }
            let left = self.operand()?;
            self.blanks();
            let op = match (self.peek(), self.peek_at(1)) {
                (Some('='), Some('=')) => Some((Op::Eq, 2)),
                (Some('!'), Some('=')) => Some((Op::Ne, 2)),
                (Some('<'), Some('=')) => Some((Op::Le, 2)),
                (Some('>'), Some('=')) => Some((Op::Ge, 2)),
                (Some('<'), _) => Some((Op::Lt, 1)),
                (Some('>'), _) => Some((Op::Gt, 1)),
                (Some('='), _) => return self.error("expected `==`"),
                _ => None,
            };
            let Some((op, width)) = op else {
                return Ok(Expr::Exists(left));
            };
            self.pos += width;
            self.blanks();
            let right = self.operand()?;
            Ok(Expr::Compare(left, op, right))
        }

        /// A singular path from `@`/`$`, or a literal.
        fn operand(&mut self) -> Result<Operand, SelectError> {
            self.blanks();
            match self.peek() {
                Some(anchor @ ('@' | '$')) => {
                    self.pos += 1;
                    let mut keys = Vec::new();
                    let mut negatives = Vec::new();
                    loop {
                        match self.peek() {
                            Some('.') if self.peek_at(1) != Some('.') => {
                                self.pos += 1;
                                keys.push(PathSegment::Key(self.name()?));
                            }
                            Some('[') => {
                                self.pos += 1;
                                self.blanks();
                                match self.peek() {
                                    Some('\'' | '"') => keys.push(PathSegment::Key(self.string()?)),
                                    _ => {
                                        let index = self.integer()?;
                                        if index < 0 {
                                            negatives.push(keys.len() as i64);
                                        }
                                        keys.push(
                                            PathSegment::Index(index.unsigned_abs() as usize),
                                        );
                                    }
                                }
                                self.blanks();
                                self.expect(']')?;
                            }
                            _ => break,
                        }
                    }
                    Ok(if anchor == '@' {
                        Operand::Current(keys, negatives)
                    } else {
                        Operand::Root(keys, negatives)
                    })
                }
                Some('\'' | '"') => Ok(Operand::Literal(Node::new(Value::String(self.string()?)))),
                Some(c) if c == '-' || c.is_ascii_digit() => {
                    let start = self.pos;
                    self.pos += 1;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || ".eE+-".contains(c))
                    {
                        self.pos += 1;
                    }
                    let text: String = self.chars[start..self.pos].iter().collect();
                    let value = if let Ok(i) = text.parse::<i64>() {
                        Value::Int(i)
                    } else if let Ok(f) = text.parse::<f64>() {
                        Value::Float(f)
                    } else {
                        self.pos = start;
                        return self.error(format!("invalid number `{text}`"));
                    };
                    Ok(Operand::Literal(Node::new(value)))
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    let start = self.pos;
                    while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                        self.pos += 1;
                    }
                    let word: String = self.chars[start..self.pos].iter().collect();
                    let value = match word.as_str() {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        "null" => Value::Null,
                        _ => {
                            self.pos = start;
                            return self.error(format!(
                                "unexpected `{word}`; expected `@`, `$` or a literal"
                            ));
                        }
                    };
                    Ok(Operand::Literal(Node::new(value)))
                }
                Some(c) => self.error(format!("unexpected `{c}` in filter")),
                None => self.error("unexpected end of filter"),
            }
        }
    }

    impl JsonPathSelector {
        /// Compile `expr`.
        pub fn parse(expr: &str) -> Result<Self, SelectError> {
            let mut parser = Parser {
                chars: expr.chars().collect(),
                pos: 0,
            };
            if parser.chars.iter().all(|c| c.is_whitespace()) {
                return parser.error("empty expression");
            }
            Ok(JsonPathSelector {
                steps: parser.path()?,
            })
        }
    }

    fn children<'a>(path: &Path, node: &'a Node) -> Vec<(Path, &'a Node)> {
        match &node.value {
            Value::Seq(items) => items
                .iter()
                .enumerate()
                .map(|(i, item)| (path.child_index(i), item))
                .collect(),
            Value::Map(entries) => entries
                .iter()
                .map(|(k, v)| (path.child_key(k), v))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn normalize(index: i64, len: usize) -> i64 {
        if index < 0 {
            len as i64 + index
        } else {
            index
        }
    }

    fn resolve<'a>(root: &'a Node, keys: &[PathSegment], negatives: &[i64]) -> Option<&'a Node> {
        let mut node = root;
        for (i, key) in keys.iter().enumerate() {
            node = match key {
                PathSegment::Key(k) => node.get(k)?,
                PathSegment::Index(n) => {
                    let n = if negatives.contains(&(i as i64)) {
                        normalize(-(*n as i64), node.len())
                    } else {
                        *n as i64
                    };
                    node.index(usize::try_from(n).ok()?)?
                }
            };
        }
        Some(node)
    }

    fn operand<'a>(operand: &'a Operand, current: &'a Node, root: &'a Node) -> Option<&'a Node> {
        match operand {
            Operand::Current(keys, negatives) => resolve(current, keys, negatives),
            Operand::Root(keys, negatives) => resolve(root, keys, negatives),
            Operand::Literal(node) => Some(node),
        }
    }

    fn number(value: &Value) -> Option<f64> {
        match value {
            Value::Int(i) => Some(*i as f64),
            Value::UInt(u) => Some(*u as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    fn order(a: &Node, b: &Node) -> Option<Ordering> {
        match (&a.value, &b.value) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
            (x, y) => number(x)?.partial_cmp(&number(y)?),
        }
    }

    fn compare(a: Option<&Node>, op: Op, b: Option<&Node>) -> bool {
        let equal = match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => match (number(&a.value), number(&b.value)) {
                (Some(x), Some(y)) => order(a, b) == Some(Ordering::Equal) || x == y,
                _ => value_eq(a, b),
            },
            _ => false,
        };
        let ordering = match (a, b) {
            (Some(a), Some(b)) => order(a, b),
            _ => None,
        };
        match op {
            Op::Eq => equal,
            Op::Ne => !equal,
            Op::Lt => ordering == Some(Ordering::Less),
            Op::Gt => ordering == Some(Ordering::Greater),
            Op::Le => ordering == Some(Ordering::Less) || (ordering.is_some() && equal),
            Op::Ge => ordering == Some(Ordering::Greater) || (ordering.is_some() && equal),
        }
    }

    fn eval(expr: &Expr, current: &Node, root: &Node) -> bool {
        match expr {
            Expr::Or(a, b) => eval(a, current, root) || eval(b, current, root),
            Expr::And(a, b) => eval(a, current, root) && eval(b, current, root),
            Expr::Not(a) => !eval(a, current, root),
            Expr::Exists(o) => match o {
                Operand::Literal(node) => node.value == Value::Bool(true),
                _ => operand(o, current, root).is_some(),
            },
            Expr::Compare(a, op, b) => {
                compare(operand(a, current, root), *op, operand(b, current, root))
            }
        }
    }

    fn apply<'a>(
        sel: &Sel,
        path: &Path,
        node: &'a Node,
        root: &'a Node,
        out: &mut Vec<(Path, &'a Node)>,
    ) {
        match sel {
            Sel::Name(name) => {
                if let Value::Map(entries) = &node.value {
                    if let Some((k, v)) = entries.iter().rev().find(|(k, _)| k == name) {
                        out.push((path.child_key(k), v));
                    }
                }
            }
            Sel::Wildcard => out.extend(children(path, node)),
            Sel::Index(index) => {
                if let Value::Seq(items) = &node.value {
                    let i = normalize(*index, items.len());
                    if let Some(item) = usize::try_from(i).ok().and_then(|i| items.get(i)) {
                        out.push((path.child_index(i as usize), item));
                    }
                }
            }
            Sel::Slice(start, end, step) => {
                if let Value::Seq(items) = &node.value {
                    let len = items.len() as i64;
                    let step = *step;
                    let clamp = |v: i64, lo: i64, hi: i64| v.max(lo).min(hi);
                    let mut push = |i: i64| {
                        out.push((path.child_index(i as usize), &items[i as usize]));
                    };
                    if step > 0 {
                        let lower = clamp(normalize(start.unwrap_or(0), items.len()), 0, len);
                        let upper = clamp(normalize(end.unwrap_or(len), items.len()), 0, len);
                        let mut i = lower;
                        while i < upper {
                            push(i);
                            i += step;
                        }
                    } else {
                        let upper = clamp(
                            normalize(start.unwrap_or(len - 1), items.len()),
                            -1,
                            len - 1,
                        );
                        let lower =
                            clamp(normalize(end.unwrap_or(-len - 1), items.len()), -1, len - 1);
                        let mut i = upper;
                        while lower < i {
                            push(i);
                            i += step;
                        }
                    }
                }
            }
            Sel::Filter(expr) => {
                for (child_path, child) in children(path, node) {
                    if eval(expr, child, root) {
                        out.push((child_path, child));
                    }
                }
            }
        }
    }

    impl Selector for JsonPathSelector {
        fn select<'a>(&self, root: &'a Node) -> Result<Vec<(Path, &'a Node)>, SelectError> {
            let mut current: Vec<(Path, &'a Node)> = vec![(Path::root(), root)];
            for step in &self.steps {
                let mut next = Vec::new();
                for (path, node) in &current {
                    let targets: Vec<(Path, &'a Node)> = if step.descendant {
                        // The node and all its descendants, parents first.
                        let mut all = Vec::new();
                        let mut stack = vec![(path.clone(), *node)];
                        while let Some((p, n)) = stack.pop() {
                            let mut kids = children(&p, n);
                            kids.reverse();
                            all.push((p, n));
                            stack.extend(kids);
                        }
                        all
                    } else {
                        vec![(path.clone(), *node)]
                    };
                    for (target_path, target) in &targets {
                        for sel in &step.selectors {
                            apply(sel, target_path, target, root, &mut next);
                        }
                    }
                }
                current = next;
            }
            Ok(current)
        }
    }
}
