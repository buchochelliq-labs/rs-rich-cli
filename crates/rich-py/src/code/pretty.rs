//! `rich.pretty`: `Pretty`, `Node`, `traverse`, `pretty_repr`, `pprint` and
//! `install`.
//!
//! Upstream builds a tree of `Node`s by walking a Python object (containers,
//! dataclasses, attrs classes, named tuples and `__rich_repr__`), then lays
//! it out, expanding a container onto several lines only when it does not
//! fit. Core's `Pretty` formats a Rust value's `Debug` output instead, and
//! cannot see Python objects, so both halves live here: the walk is the
//! binding's reflection of Python values, the layout is a line-for-line port
//! of upstream's `Node.render` and `_Line`.

use std::collections::HashSet;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBytes, PyDict, PyList, PySlice, PyString, PyTuple, PyType};

use rich::cells::cell_len;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::Text as CoreText;

use super::highlighter::Highlight;
use crate::convert;
use crate::renderable::{self, AsRenderable, PyRenderable};

// ---------------------------------------------------------------------------
// Node and its layout (upstream `Node` and `_Line`)

/// A node in a repr tree: an atom (`value_repr`) or a container.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) key_repr: String,
    pub(crate) value_repr: String,
    pub(crate) open_brace: String,
    pub(crate) close_brace: String,
    pub(crate) empty: String,
    pub(crate) last: bool,
    pub(crate) is_tuple: bool,
    pub(crate) is_namedtuple: bool,
    pub(crate) children: Children,
    pub(crate) key_separator: String,
    pub(crate) separator: String,
}

/// A node's children (`None` for an atom). Cloned and dropped without
/// recursion, so a deep tree cannot overflow the native stack.
#[derive(Debug, Default)]
pub(crate) struct Children(pub(crate) Option<Vec<Node>>);

impl Clone for Children {
    fn clone(&self) -> Self {
        Children(
            self.0
                .as_ref()
                .map(|children| children.iter().map(Node::deep_clone).collect()),
        )
    }
}

impl From<Option<Vec<Node>>> for Children {
    fn from(children: Option<Vec<Node>>) -> Self {
        Children(children)
    }
}

impl std::ops::Deref for Children {
    type Target = Option<Vec<Node>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Children {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        let Some(children) = self.0.take() else {
            return;
        };
        // Each node's children move onto this list before the node drops,
        // so no drop goes deeper than one level.
        let mut pending = children;
        while let Some(mut node) = pending.pop() {
            if let Some(grandchildren) = node.children.0.take() {
                pending.extend(grandchildren);
            }
        }
    }
}

impl Default for Node {
    fn default() -> Self {
        Node {
            key_repr: String::new(),
            value_repr: String::new(),
            open_brace: String::new(),
            close_brace: String::new(),
            empty: String::new(),
            last: false,
            is_tuple: false,
            is_namedtuple: false,
            children: Children(None),
            key_separator: ": ".to_string(),
            separator: ", ".to_string(),
        }
    }
}

impl Node {
    /// The node without its children.
    fn shallow_clone(&self) -> Node {
        Node {
            key_repr: self.key_repr.clone(),
            value_repr: self.value_repr.clone(),
            open_brace: self.open_brace.clone(),
            close_brace: self.close_brace.clone(),
            empty: self.empty.clone(),
            last: self.last,
            is_tuple: self.is_tuple,
            is_namedtuple: self.is_namedtuple,
            children: Children(None),
            key_separator: self.key_separator.clone(),
            separator: self.separator.clone(),
        }
    }

    /// A copy of the whole tree, built without recursion.
    fn deep_clone(&self) -> Node {
        enum Step<'a> {
            Enter(&'a Node),
            Exit(&'a Node, usize),
        }
        let mut stack = vec![Step::Enter(self)];
        let mut built: Vec<Node> = Vec::new();
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter(node) => match node.children.as_ref() {
                    None => built.push(node.shallow_clone()),
                    Some(children) => {
                        stack.push(Step::Exit(node, children.len()));
                        stack.extend(children.iter().rev().map(Step::Enter));
                    }
                },
                Step::Exit(node, count) => {
                    let children = built.split_off(built.len() - count);
                    let mut copy = node.shallow_clone();
                    copy.children = Children(Some(children));
                    built.push(copy);
                }
            }
        }
        built.pop().expect("the root is built last")
    }

    /// Whether two trees are equal (`Node.__eq__`), compared without
    /// recursion.
    fn same(&self, other: &Node) -> bool {
        let mut pairs = vec![(self, other)];
        while let Some((a, b)) = pairs.pop() {
            let fields_equal = a.key_repr == b.key_repr
                && a.value_repr == b.value_repr
                && a.open_brace == b.open_brace
                && a.close_brace == b.close_brace
                && a.empty == b.empty
                && a.last == b.last
                && a.is_tuple == b.is_tuple
                && a.is_namedtuple == b.is_namedtuple
                && a.key_separator == b.key_separator
                && a.separator == b.separator;
            if !fields_equal {
                return false;
            }
            match (a.children.as_ref(), b.children.as_ref()) {
                (None, None) => {}
                (Some(left), Some(right)) if left.len() == right.len() => {
                    pairs.extend(left.iter().zip(right));
                }
                _ => return false,
            }
        }
        true
    }

    fn value(value_repr: impl Into<String>) -> Node {
        Node {
            value_repr: value_repr.into(),
            ..Node::default()
        }
    }

    /// `Node.iter_tokens`, as a visitor that stops when `f` returns false.
    /// Returns false when stopped. Iterative, so a deep tree cannot
    /// overflow the native stack.
    fn tokens<'a>(&'a self, f: &mut dyn FnMut(&'a str) -> bool) -> bool {
        enum Step<'a> {
            Node(&'a Node),
            Token(&'a str),
        }
        let mut stack = vec![Step::Node(self)];
        while let Some(step) = stack.pop() {
            let node = match step {
                Step::Token(token) => {
                    if !f(token) {
                        return false;
                    }
                    continue;
                }
                Step::Node(node) => node,
            };
            if !node.key_repr.is_empty() && !(f(&node.key_repr) && f(&node.key_separator)) {
                return false;
            }
            if !node.value_repr.is_empty() {
                if !f(&node.value_repr) {
                    return false;
                }
                continue;
            }
            let Some(children) = node.children.as_ref() else {
                continue;
            };
            if children.is_empty() {
                if !f(&node.empty) {
                    return false;
                }
                continue;
            }
            if !f(&node.open_brace) {
                return false;
            }
            // Pushed in reverse: children and separators, then the brace.
            stack.push(Step::Token(&node.close_brace));
            if node.is_tuple && !node.is_namedtuple && children.len() == 1 {
                stack.push(Step::Token(","));
                stack.push(Step::Node(&children[0]));
            } else {
                for child in children.iter().rev() {
                    if !child.last {
                        stack.push(Step::Token(&node.separator));
                    }
                    stack.push(Step::Node(child));
                }
            }
        }
        true
    }

    pub(crate) fn token_list(&self) -> Vec<String> {
        let mut tokens = Vec::new();
        self.tokens(&mut |token| {
            tokens.push(token.to_string());
            true
        });
        tokens
    }

    /// `Node.check_length`: whether the node fits in `max_length` cells
    /// after `start_length`.
    pub(crate) fn check_length(&self, start_length: isize, max_length: isize) -> bool {
        let mut total = start_length;
        self.tokens(&mut |token| {
            total += cell_len(token) as isize;
            total <= max_length
        })
    }

    /// `str(node)`: the one-line repr.
    pub(crate) fn to_repr(&self) -> String {
        let mut repr = String::new();
        self.tokens(&mut |token| {
            repr.push_str(token);
            true
        });
        repr
    }

    /// Refuse an `indent_size` Rich would run out of memory indenting with
    /// (a line expands to `" " * indent_size` more per level).
    pub(crate) fn check_indent(&self, indent_size: usize) -> PyResult<()> {
        if self
            .children
            .as_ref()
            .is_some_and(|children| !children.is_empty())
        {
            crate::limits::check_alloc(
                "indent_size",
                indent_size,
                crate::limits::MAX_CONSOLE_WIDTH,
            )?;
        }
        Ok(())
    }

    /// `Node.render`: the repr, expanded onto new lines to fit `max_width`.
    pub(crate) fn render(&self, max_width: isize, indent_size: usize, expand_all: bool) -> String {
        let mut lines = vec![Line {
            node: Some(self),
            ..Line::default()
        }];
        let mut line_no = 0;
        while line_no < lines.len() {
            let line = &lines[line_no];
            if line.expandable() && (expand_all || !line.check_length(max_width)) {
                let expanded = line.expand(indent_size);
                lines.splice(line_no..line_no + 1, expanded);
            }
            line_no += 1;
        }
        lines
            .iter()
            .map(Line::to_repr)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Upstream's `_Line`: one line of repr output.
#[derive(Default)]
struct Line<'a> {
    node: Option<&'a Node>,
    text: String,
    suffix: String,
    whitespace: String,
    last: bool,
}

impl<'a> Line<'a> {
    fn expandable(&self) -> bool {
        self.node
            .and_then(|node| node.children.as_ref())
            .is_some_and(|children| !children.is_empty())
    }

    fn check_length(&self, max_length: isize) -> bool {
        let start = self.whitespace.len() + cell_len(&self.text) + cell_len(&self.suffix);
        self.node
            .expect("an expandable line has a node")
            .check_length(start as isize, max_length)
    }

    fn expand(&self, indent_size: usize) -> Vec<Line<'a>> {
        let node = self.node.expect("an expandable line has a node");
        let children = node.children.as_deref().unwrap_or_default();
        let mut lines = Vec::with_capacity(children.len() + 2);
        let text = if node.key_repr.is_empty() {
            node.open_brace.clone()
        } else {
            format!("{}{}{}", node.key_repr, node.key_separator, node.open_brace)
        };
        lines.push(Line {
            text,
            whitespace: self.whitespace.clone(),
            ..Line::default()
        });
        let child_whitespace = format!("{}{}", self.whitespace, " ".repeat(indent_size));
        let tuple_of_one = node.is_tuple && children.len() == 1;
        let last_index = children.len().saturating_sub(1);
        for (index, child) in children.iter().enumerate() {
            let separator = if tuple_of_one { "," } else { &node.separator };
            lines.push(Line {
                node: Some(child),
                whitespace: child_whitespace.clone(),
                suffix: separator.to_string(),
                last: index == last_index && !tuple_of_one,
                ..Line::default()
            });
        }
        lines.push(Line {
            text: node.close_brace.clone(),
            whitespace: self.whitespace.clone(),
            suffix: self.suffix.clone(),
            last: self.last,
            ..Line::default()
        });
        lines
    }

    fn to_repr(&self) -> String {
        let node = self.node.map(Node::to_repr).unwrap_or_default();
        if self.last {
            format!("{}{}{}", self.whitespace, self.text, node)
        } else {
            format!(
                "{}{}{}{}",
                self.whitespace,
                self.text,
                node,
                self.suffix.trim_end()
            )
        }
    }
}

// ---------------------------------------------------------------------------
// traverse: a Python object to a Node tree

/// Limits for [`traverse`].
#[derive(Clone, Copy, Default)]
pub(crate) struct Limits {
    pub(crate) max_length: Option<usize>,
    pub(crate) max_string: Option<usize>,
    pub(crate) max_depth: Option<usize>,
}

/// Python helpers the walk needs, looked up once.
struct Helpers {
    containers: Vec<Py<PyType>>,
    mappings: Py<PyTuple>,
    dataclass_files: Vec<Py<PyAny>>,
    namedtuple_file: Py<PyAny>,
    attr: Option<Py<PyModule>>,
}

fn helpers(py: Python<'_>) -> PyResult<&Helpers> {
    static HELPERS: PyOnceLock<Helpers> = PyOnceLock::new();
    HELPERS.get_or_try_init(py, || {
        let builtins = py.import("builtins")?;
        let collections = py.import("collections")?;
        let os = py.import("os")?;
        let mapping_proxy = py.import("types")?.getattr("MappingProxyType")?;
        // Upstream's `_BRACES` order: the first class an object is an
        // instance of picks its braces.
        let containers = [
            os.getattr("_Environ")?,
            py.import("array")?.getattr("array")?,
            collections.getattr("defaultdict")?,
            collections.getattr("Counter")?,
            collections.getattr("deque")?,
            builtins.getattr("dict")?,
            collections.getattr("UserDict")?,
            builtins.getattr("frozenset")?,
            builtins.getattr("list")?,
            collections.getattr("UserList")?,
            builtins.getattr("set")?,
            builtins.getattr("tuple")?,
            mapping_proxy.clone(),
        ]
        .into_iter()
        .map(|class| Ok(class.cast_into::<PyType>()?.unbind()))
        .collect::<PyResult<Vec<_>>>()?;
        let mappings = PyTuple::new(
            py,
            [
                builtins.getattr("dict")?,
                os.getattr("_Environ")?,
                mapping_proxy,
                collections.getattr("UserDict")?,
            ],
        )?
        .unbind();
        let dataclass_files = vec![
            py.import("dataclasses")?.getattr("__file__")?.unbind(),
            py.import("reprlib")?.getattr("__file__")?.unbind(),
        ];
        let dummy =
            collections.call_method1("namedtuple", ("_dummy_namedtuple", PyList::empty(py)))?;
        let namedtuple_file = py
            .import("inspect")?
            .call_method1("getfile", (dummy.getattr("__repr__")?,))?
            .unbind();
        let attr = match py.import("attr") {
            Ok(module) if module.hasattr("ib")? => Some(module.unbind()),
            _ => None,
        };
        Ok(Helpers {
            containers,
            mappings,
            dataclass_files,
            namedtuple_file,
            attr,
        })
    })
}

fn safe_isinstance(object: &Bound<'_, PyAny>, class: &Bound<'_, PyAny>) -> bool {
    object.is_instance(class).unwrap_or(false)
}

fn is_class(object: &Bound<'_, PyAny>) -> bool {
    object.is_instance_of::<PyType>()
}

/// `_is_namedtuple`: a tuple with a tuple `_fields`.
fn is_namedtuple(object: &Bound<'_, PyAny>) -> bool {
    let fields = match object.getattr_opt("_fields") {
        Ok(Some(fields)) => fields,
        _ => return false,
    };
    object.is_instance_of::<PyTuple>() && fields.is_instance_of::<PyTuple>()
}

/// One attrs field: its name, its value (or the error getting it), and
/// its own repr function.
type AttrItem<'py> = (
    String,
    Result<Bound<'py, PyAny>, PyErr>,
    Option<Bound<'py, PyAny>>,
);

struct Walker<'a, 'py> {
    py: Python<'py>,
    limits: Limits,
    helpers: &'a Helpers,
    visited: HashSet<usize>,
    /// The depth at which upstream's recursive walk runs out of Python
    /// frames: the node there is a `<repr-error ...>` (see [`traverse_with`]).
    /// Found when the walk first gets deep ([`Walker::repr_error_depth`]).
    repr_error_depth: Option<usize>,
    /// The Python frames upstream's callers take above its walk.
    frames: usize,
    /// Whether the walk is on the caller's stack, and so moves to a thread
    /// of its own past [`STACK_BUDGET`] (see [`Walker::walk_on_new_stack`]).
    on_caller_stack: bool,
    /// Where the walk started on the caller's stack.
    stack_base: usize,
    /// The thread with a big stack the walk continues on, once started.
    worker: Option<Worker>,
}

/// A part of a walk for the [`Worker`]: walk `object`, with the state so far.
struct Job {
    object: Py<PyAny>,
    root: bool,
    depth: usize,
    visited: HashSet<usize>,
}

/// A [`Job`] done: its node, and the state after it.
struct Reply {
    node: PyResult<Node>,
    visited: HashSet<usize>,
}

/// A thread with a stack of [`HOP_STACK_SIZE`] that walks the parts of an
/// object too deep for the caller's stack. It lives as long as the walk,
/// and runs only while the caller waits for it (the GIL passes between
/// them), so the walk still runs one step at a time, in order.
struct Worker {
    jobs: Option<std::sync::mpsc::Sender<Job>>,
    replies: std::sync::mpsc::Receiver<Reply>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn spawn(limits: Limits, repr_error_depth: usize) -> std::io::Result<Worker> {
        let (jobs, job_queue) = std::sync::mpsc::channel::<Job>();
        let (reply_queue, replies) = std::sync::mpsc::channel::<Reply>();
        let thread = std::thread::Builder::new()
            .name("rs_rich-pretty".to_string())
            .stack_size(HOP_STACK_SIZE)
            .spawn(move || {
                for job in job_queue {
                    let reply = Python::attach(|py| {
                        let Job {
                            object,
                            root,
                            depth,
                            visited,
                        } = job;
                        let helpers = match helpers(py) {
                            Ok(helpers) => helpers,
                            Err(error) => {
                                return Reply {
                                    node: Err(error),
                                    visited,
                                }
                            }
                        };
                        let mut walker = Walker {
                            py,
                            limits,
                            helpers,
                            visited,
                            repr_error_depth: Some(repr_error_depth),
                            frames: 0,
                            on_caller_stack: false,
                            stack_base: 0,
                            worker: None,
                        };
                        let node = walker.walk(object.bind(py), root, depth);
                        Reply {
                            node,
                            visited: std::mem::take(&mut walker.visited),
                        }
                    });
                    if reply_queue.send(reply).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Worker {
            jobs: Some(jobs),
            replies,
            thread: Some(thread),
        })
    }

    /// Hand `job` to the thread and wait for it (without the GIL).
    fn run(&self, job: Job) -> Option<Reply> {
        self.jobs.as_ref()?.send(job).ok()?;
        self.replies.recv().ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the queue ends the thread, which holds nothing by now.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How much of the caller's native stack, of unknown size, the walk uses
/// before it continues on a thread with a stack of [`HOP_STACK_SIZE`]: the
/// walk recurses (as upstream's does, in Python), and a deep enough object
/// would otherwise overflow the stack and kill the interpreter.
const STACK_BUDGET: usize = 192 << 10;

/// The stack of the thread a deep walk continues on: room for the rest of
/// [`MAX_PRETTY_DEPTH`] levels, and the Python code (`__repr__`, ABC
/// checks) each runs. Only what is used is ever committed.
const HOP_STACK_SIZE: usize = 256 << 20;

/// An address on the current native stack, to measure how much is used.
#[inline(never)]
fn stack_position() -> usize {
    let marker = 0u8;
    std::hint::black_box(std::ptr::addr_of!(marker)) as usize
}

/// The deepest node the walk builds, whatever the recursion limit: past it
/// the node is a `<repr-error ...>`, as upstream's is when it runs out of
/// frames (with the default recursion limit, a little under 1000 levels).
/// Laying out and dropping a tree recurses in places, so this keeps it well
/// inside a small native stack.
const MAX_PRETTY_DEPTH: usize = 3000;

/// How deep a walk goes before it looks for the depth upstream's walk
/// would run out of frames at: well short of any recursion limit in use.
const SHALLOW_DEPTH: usize = 50;

/// The node upstream builds where its recursive walk exceeds the recursion
/// limit: `repr(obj)` raises `RecursionError`, which `to_repr` reports.
const RECURSION_REPR_ERROR: &str =
    "<repr-error 'maximum recursion depth exceeded while getting the repr of an object'>";

impl<'py> Walker<'_, 'py> {
    /// `to_repr`: the repr, with long strings cut at `max_string`, and a
    /// failing `__repr__` reported rather than raised.
    fn to_repr(&self, object: &Bound<'py, PyAny>) -> PyResult<String> {
        if let Some(max_string) = self.limits.max_string {
            if object.is_instance_of::<PyString>() || object.is_instance_of::<PyBytes>() {
                let length = object.len()?;
                if length > max_string {
                    let head = object.get_item(PySlice::new(self.py, 0, max_string as isize, 1))?;
                    return Ok(format!("{}+{}", head.repr()?, length - max_string));
                }
            }
        }
        match object.repr() {
            Ok(repr) => Ok(repr.to_string()),
            Err(error) => {
                let message = error.value(self.py).str()?;
                Ok(format!("<repr-error {}>", message.repr()?))
            }
        }
    }

    fn is_attr_object(&self, object: &Bound<'py, PyAny>) -> bool {
        let Some(attr) = &self.helpers.attr else {
            return false;
        };
        attr.bind(self.py)
            .call_method1("has", (object.get_type(),))
            .and_then(|has| has.is_truthy())
            .unwrap_or(false)
    }

    fn is_dataclass_repr(&self, object: &Bound<'py, PyAny>) -> bool {
        let file = (|| {
            object
                .getattr("__repr__")?
                .getattr("__code__")?
                .getattr("co_filename")
        })();
        match file {
            Ok(file) => self
                .helpers
                .dataclass_files
                .iter()
                .any(|known| file.eq(known.bind(self.py)).unwrap_or(false)),
            Err(_) => false,
        }
    }

    fn has_default_namedtuple_repr(&self, object: &Bound<'py, PyAny>) -> PyResult<bool> {
        let file = object
            .getattr("__repr__")
            .and_then(|repr| self.py.import("inspect")?.call_method1("getfile", (repr,)));
        Ok(match file {
            Ok(file) => file.eq(self.helpers.namedtuple_file.bind(self.py))?,
            Err(error)
                if error.is_instance_of::<pyo3::exceptions::PyOSError>(self.py)
                    || error.is_instance_of::<PyTypeError>(self.py) =>
            {
                false
            }
            Err(error) => return Err(error),
        })
    }

    /// The depth at which upstream's walk, called where this one was, runs
    /// out of Python frames: at most [`MAX_PRETTY_DEPTH`]. Found (on the
    /// caller's thread) the first time the walk needs it, which only a deep
    /// object does.
    fn repr_error_depth(&mut self) -> PyResult<usize> {
        if let Some(depth) = self.repr_error_depth {
            return Ok(depth);
        }
        let left = python_frames_left(self.py)?;
        // `probe` and `down(0)` take two frames, and the failing call one;
        // `frames` are upstream's.
        let depth = (left + 3).saturating_sub(self.frames).min(MAX_PRETTY_DEPTH);
        self.repr_error_depth = Some(depth);
        Ok(depth)
    }

    /// Walk `object` (at `depth`) on the walk's own thread, which has a big
    /// stack, with the GIL handed over to it, and come back with the node.
    /// The thread starts the first time it is needed and serves the rest of
    /// the walk; the walk's state moves there and back.
    fn walk_on_new_stack(
        &mut self,
        object: &Bound<'py, PyAny>,
        root: bool,
        depth: usize,
    ) -> PyResult<Node> {
        let worker = match self.worker.take() {
            Some(worker) => worker,
            // Found here: the thread's Python stack is not the caller's.
            None => Worker::spawn(self.limits, self.repr_error_depth()?).map_err(|_| {
                pyo3::exceptions::PyRecursionError::new_err(
                    "maximum recursion depth exceeded while pretty printing",
                )
            })?,
        };
        let job = Job {
            object: object.clone().unbind(),
            root,
            depth,
            visited: std::mem::take(&mut self.visited),
        };
        let (worker, reply) = self.py.detach(move || {
            let reply = worker.run(job);
            (worker, reply)
        });
        let mut worker = worker;
        let reply = match reply {
            Some(reply) => reply,
            // The thread is gone: it panicked (carry the panic on).
            None => match worker.thread.take().map(|thread| thread.join()) {
                Some(Err(panic)) => std::panic::resume_unwind(panic),
                _ => {
                    return Err(pyo3::exceptions::PyRuntimeError::new_err(
                        "the pretty printer's thread stopped",
                    ))
                }
            },
        };
        self.worker = Some(worker);
        self.visited = reply.visited;
        reply.node
    }

    /// Mark `children` so the last one knows it is last (upstream's
    /// `loop_last`).
    fn keyed(&mut self, key: String, child: &Bound<'py, PyAny>, depth: usize) -> PyResult<Node> {
        let mut node = self.walk(child, false, depth + 1)?;
        node.key_repr = key;
        node.key_separator = "=".to_string();
        Ok(node)
    }

    fn walk(&mut self, object: &Bound<'py, PyAny>, root: bool, depth: usize) -> PyResult<Node> {
        let py = self.py;
        let id = object.as_ptr() as usize;
        if self.visited.contains(&id) {
            return Ok(Node::value("..."));
        }
        if depth >= SHALLOW_DEPTH && depth >= self.repr_error_depth()? {
            let mut node = Node::value(RECURSION_REPR_ERROR);
            node.last = root;
            return Ok(node);
        }
        if self.on_caller_stack
            && !is_atom(object)
            && stack_position().abs_diff(self.stack_base) > STACK_BUDGET
        {
            return self.walk_on_new_stack(object, root, depth);
        }
        let reached_max_depth = self.limits.max_depth.is_some_and(|max| depth >= max);
        let fake_attributes = object
            .hasattr("awehoi234_wdfjwljet234_234wdfoijsdfmmnxpi492")
            .unwrap_or(false);
        let class_name =
            || -> PyResult<String> { object.getattr("__class__")?.getattr("__name__")?.extract() };

        let mut rich_repr_result = None;
        if !fake_attributes {
            if let Ok(true) = object.hasattr("__rich_repr__") {
                if !is_class(object) {
                    if let Ok(result) = object.call_method0("__rich_repr__") {
                        if !result.is_none() {
                            rich_repr_result = Some(result);
                        }
                    }
                }
            }
        }

        let mut node = if let Some(result) = rich_repr_result {
            self.visited.insert(id);
            let angular = object
                .getattr("__rich_repr__")?
                .getattr_opt("angular")?
                .map(|a| a.is_truthy())
                .transpose()?
                .unwrap_or(false);
            // `iter_rich_args`.
            let mut args: Vec<(Option<String>, Bound<'py, PyAny>)> = Vec::new();
            for arg in result.try_iter()? {
                let arg = arg?;
                if let Ok(tuple) = arg.cast::<PyTuple>() {
                    match tuple.len() {
                        3 => {
                            let (key, child, default) =
                                (tuple.get_item(0)?, tuple.get_item(1)?, tuple.get_item(2)?);
                            if default.eq(&child)? {
                                continue;
                            }
                            args.push((Some(key.extract()?), child));
                        }
                        2 => args.push((Some(tuple.get_item(0)?.extract()?), tuple.get_item(1)?)),
                        1 => args.push((None, tuple.get_item(0)?)),
                        _ => {}
                    }
                } else {
                    args.push((None, arg));
                }
            }
            let class_name = class_name()?;
            let node = if args.is_empty() {
                Node {
                    value_repr: if angular {
                        format!("<{class_name}>")
                    } else {
                        format!("{class_name}()")
                    },
                    children: Some(Vec::new()).into(),
                    last: root,
                    ..Node::default()
                }
            } else if reached_max_depth {
                Node::value(if angular {
                    format!("<{class_name}...>")
                } else {
                    format!("{class_name}(...)")
                })
            } else {
                let last_index = args.len() - 1;
                let mut children = Vec::with_capacity(args.len());
                for (index, (key, child)) in args.iter().enumerate() {
                    let mut child_node = match key {
                        Some(key) => self.keyed(key.clone(), child, depth)?,
                        None => self.walk(child, false, depth + 1)?,
                    };
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                if angular {
                    Node {
                        open_brace: format!("<{class_name} "),
                        close_brace: ">".to_string(),
                        children: Some(children).into(),
                        last: root,
                        separator: " ".to_string(),
                        ..Node::default()
                    }
                } else {
                    Node {
                        open_brace: format!("{class_name}("),
                        close_brace: ")".to_string(),
                        children: Some(children).into(),
                        last: root,
                        ..Node::default()
                    }
                }
            };
            self.visited.remove(&id);
            node
        } else if self.is_attr_object(object) && !fake_attributes {
            self.visited.insert(id);
            let attr = self
                .helpers
                .attr
                .as_ref()
                .expect("attrs is installed")
                .bind(py);
            let fields = attr.call_method1("fields", (object.get_type(),))?;
            let node = if fields.len()? == 0 {
                Node {
                    value_repr: format!("{}()", class_name()?),
                    children: Some(Vec::new()).into(),
                    last: root,
                    ..Node::default()
                }
            } else if reached_max_depth {
                Node::value(format!("{}(...)", class_name()?))
            } else {
                // `iter_attrs`.
                let mut items: Vec<AttrItem<'py>> = Vec::new();
                for field in fields.try_iter()? {
                    let field = field?;
                    let repr = field.getattr("repr")?;
                    if !repr.is_truthy()? {
                        continue;
                    }
                    let name: String = field.getattr("name")?.extract()?;
                    let value = object.getattr(name.as_str());
                    let callable = if value.is_ok() && repr.is_callable() {
                        Some(repr)
                    } else {
                        None
                    };
                    items.push((name, value, callable));
                }
                let last_index = items.len().saturating_sub(1);
                let mut children = Vec::with_capacity(items.len());
                for (index, (name, value, callable)) in items.into_iter().enumerate() {
                    let value = value
                        .unwrap_or_else(|error| error.into_value(py).into_bound(py).into_any());
                    let mut child_node = match callable {
                        Some(callable) => {
                            Node::value(callable.call1((&value,))?.str()?.to_string())
                        }
                        None => self.walk(&value, false, depth + 1)?,
                    };
                    child_node.last = index == last_index;
                    child_node.key_repr = name;
                    child_node.key_separator = "=".to_string();
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{}(", class_name()?),
                    close_brace: ")".to_string(),
                    children: Some(children).into(),
                    last: root,
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if py
            .import("dataclasses")?
            .call_method1("is_dataclass", (object,))?
            .is_truthy()?
            && !is_class(object)
            && !fake_attributes
            && self.is_dataclass_repr(object)
        {
            self.visited.insert(id);
            let class_name = class_name()?;
            let node = if reached_max_depth {
                Node::value(format!("{class_name}(...)"))
            } else {
                let mut fields = Vec::new();
                for field in py
                    .import("dataclasses")?
                    .call_method1("fields", (object,))?
                    .try_iter()?
                {
                    let field = field?;
                    let name: String = field.getattr("name")?.extract()?;
                    if field.getattr("repr")?.is_truthy()? && object.hasattr(name.as_str())? {
                        fields.push(name);
                    }
                }
                let last_index = fields.len().saturating_sub(1);
                let mut children = Vec::with_capacity(fields.len());
                for (index, name) in fields.into_iter().enumerate() {
                    let value = object.getattr(name.as_str())?;
                    let mut child_node = self.keyed(name, &value, depth)?;
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{class_name}("),
                    close_brace: ")".to_string(),
                    children: Some(children).into(),
                    last: root,
                    empty: format!("{class_name}()"),
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if is_namedtuple(object) && self.has_default_namedtuple_repr(object)? {
            self.visited.insert(id);
            let class_name = class_name()?;
            let node = if reached_max_depth {
                Node::value(format!("{class_name}(...)"))
            } else {
                let items = object.call_method0("_asdict")?.call_method0("items")?;
                let items: Vec<(String, Bound<'py, PyAny>)> = items
                    .try_iter()?
                    .map(|item| item?.extract())
                    .collect::<PyResult<_>>()?;
                let last_index = items.len().saturating_sub(1);
                let mut children = Vec::with_capacity(items.len());
                for (index, (key, value)) in items.into_iter().enumerate() {
                    let mut child_node = self.keyed(key, &value, depth)?;
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{class_name}("),
                    close_brace: ")".to_string(),
                    children: Some(children).into(),
                    empty: format!("{class_name}()"),
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if let Some(container) = self
            .helpers
            .containers
            .iter()
            .map(|class| class.bind(py))
            .find(|class| safe_isinstance(object, class.as_any()))
        {
            self.visited.insert(id);
            let (open_brace, close_brace, empty) = braces(object, container)?;
            let node = if reached_max_depth {
                Node::value(format!("{open_brace}...{close_brace}"))
            } else if !container
                .getattr("__repr__")?
                .eq(object.get_type().getattr("__repr__")?)?
            {
                Node {
                    value_repr: self.to_repr(object)?,
                    last: root,
                    ..Node::default()
                }
            } else if object.is_truthy()? {
                let num_items = object.len()?;
                let last_item_index = num_items as isize - 1;
                let mut children = Vec::new();
                let is_mapping = safe_isinstance(object, self.helpers.mappings.bind(py).as_any());
                let max_length = self.limits.max_length;
                if is_mapping {
                    let items = object.call_method0("items")?;
                    for (index, item) in items.try_iter()?.enumerate() {
                        if max_length.is_some_and(|max| index >= max) {
                            break;
                        }
                        let (key, child): (Bound<'py, PyAny>, Bound<'py, PyAny>) =
                            item?.extract()?;
                        let mut child_node = self.walk(&child, false, depth + 1)?;
                        child_node.key_repr = self.to_repr(&key)?;
                        child_node.last = index as isize == last_item_index;
                        children.push(child_node);
                    }
                } else {
                    for (index, child) in object.try_iter()?.enumerate() {
                        if max_length.is_some_and(|max| index >= max) {
                            break;
                        }
                        let mut child_node = self.walk(&child?, false, depth + 1)?;
                        child_node.last = index as isize == last_item_index;
                        children.push(child_node);
                    }
                }
                if let Some(max_length) = max_length {
                    if num_items > max_length {
                        children.push(Node {
                            value_repr: format!("... +{}", num_items - max_length),
                            last: true,
                            ..Node::default()
                        });
                    }
                }
                Node {
                    open_brace,
                    close_brace,
                    children: Some(children).into(),
                    last: root,
                    ..Node::default()
                }
            } else {
                Node {
                    empty,
                    children: Some(Vec::new()).into(),
                    last: root,
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else {
            Node {
                value_repr: self.to_repr(object)?,
                last: root,
                ..Node::default()
            }
        };
        node.is_tuple = object.get_type().is(py.get_type::<PyTuple>());
        node.is_namedtuple = is_namedtuple(object);
        Ok(node)
    }
}

/// Upstream's `_BRACES[type](obj)`.
fn braces(
    object: &Bound<'_, PyAny>,
    container: &Bound<'_, PyType>,
) -> PyResult<(String, String, String)> {
    let name: String = container.getattr("__qualname__")?.extract()?;
    let module: String = container.getattr("__module__")?.extract()?;
    let simple = |open: &str, close: &str, empty: &str| {
        (open.to_string(), close.to_string(), empty.to_string())
    };
    Ok(match (module.as_str(), name.as_str()) {
        ("os", "_Environ") => simple("environ({", "})", "environ({})"),
        ("array", "array") => {
            let typecode = object.getattr("typecode")?.repr()?;
            (
                format!("array({typecode}, ["),
                "])".to_string(),
                format!("array({typecode})"),
            )
        }
        ("collections", "defaultdict") => {
            let factory = object.getattr("default_factory")?.repr()?;
            (
                format!("defaultdict({factory}, {{"),
                "})".to_string(),
                format!("defaultdict({factory}, {{}})"),
            )
        }
        ("collections", "Counter") => simple("Counter({", "})", "Counter()"),
        ("collections", "deque") => {
            let maxlen = object.getattr("maxlen")?;
            if maxlen.is_none() {
                simple("deque([", "])", "deque()")
            } else {
                let maxlen = maxlen.repr()?;
                (
                    "deque([".to_string(),
                    format!("], maxlen={maxlen})"),
                    format!("deque(maxlen={maxlen})"),
                )
            }
        }
        (_, "frozenset") => simple("frozenset({", "})", "frozenset()"),
        (_, "list") | ("collections", "UserList") => simple("[", "]", "[]"),
        (_, "set") => simple("{", "}", "set()"),
        (_, "tuple") => simple("(", ")", "()"),
        (_, "mappingproxy") => simple("mappingproxy({", "})", "mappingproxy({})"),
        _ => simple("{", "}", "{}"),
    })
}

/// Python frames between a caller and upstream's `_traverse(obj, depth=0)`
/// when upstream renders an object (`Console.print`, `Pretty`): the walk
/// runs out of frames this much sooner than the recursion limit.
const RENDER_FRAMES: usize = 12;

/// `rich.pretty.traverse`, as upstream's is called to render an object.
pub(crate) fn traverse(object: &Bound<'_, PyAny>, limits: Limits) -> PyResult<Node> {
    traverse_with(object, limits, RENDER_FRAMES)
}

/// `rich.pretty.traverse`. Upstream's walk recurses in Python, so at the
/// recursion limit (less the `frames` its callers and the walk itself take)
/// the node is a `<repr-error ...>` instead; the walk here stops at the same
/// depth, and at [`MAX_PRETTY_DEPTH`] at the latest.
pub(crate) fn traverse_with(
    object: &Bound<'_, PyAny>,
    limits: Limits,
    frames: usize,
) -> PyResult<Node> {
    let py = object.py();
    let helpers = helpers(py)?;
    let mut walker = Walker {
        py,
        limits,
        helpers,
        visited: HashSet::new(),
        repr_error_depth: None,
        frames,
        on_caller_stack: true,
        stack_base: stack_position(),
        worker: None,
    };
    let _nesting = renderable::Nesting::enter()?;
    walker.walk(object, true, 0)
}

/// Whether `object` is a value the walk never goes into (it has no
/// children), so walking it needs no stack of its own.
fn is_atom(object: &Bound<'_, PyAny>) -> bool {
    object.is_none()
        || object.is_exact_instance_of::<PyString>()
        || object.is_exact_instance_of::<pyo3::types::PyInt>()
        || object.is_exact_instance_of::<pyo3::types::PyFloat>()
        || object.is_exact_instance_of::<pyo3::types::PyBool>()
        || object.is_exact_instance_of::<PyBytes>()
}

/// How many more nested Python calls fit on this thread before
/// `RecursionError`: what is left of the recursion limit here, counted as
/// Python counts it (C calls on the stack can count too).
fn python_frames_left(py: Python<'_>) -> PyResult<usize> {
    static PROBE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    let probe = PROBE.get_or_try_init(py, || {
        let module = PyModule::from_code(
            py,
            c"def probe():\n    def down(n):\n        try:\n            return down(n + 1)\n        except RecursionError:\n            return n\n    return down(0)\n",
            c"rs_rich_pretty_probe.py",
            c"rs_rich_pretty_probe",
        )?;
        Ok::<_, PyErr>(module.getattr("probe")?.unbind())
    })?;
    probe.bind(py).call0()?.extract()
}

// ---------------------------------------------------------------------------
// Text helpers upstream keeps on `Text`

/// `Text.from_ansi(s, style=style)`: core's.
pub(crate) fn from_ansi(content: &str, style: &str) -> CoreText {
    CoreText::from_ansi(content, StyleType::Name(style.to_string()))
}

/// `Text.with_indent_guides(indent_size, style=style)`: core's.
pub(crate) fn with_indent_guides(
    text: &CoreText,
    indent_size: usize,
    style: StyleType,
) -> CoreText {
    text.with_indent_guides(Some(indent_size.max(1)), "│", style)
}

// ---------------------------------------------------------------------------
// Pretty

/// What a `Pretty` lays out, whether or not its highlighter is Python code.
pub(crate) struct Layout {
    pub(crate) node: Node,
    pub(crate) type_repr: String,
    pub(crate) indent_size: usize,
    pub(crate) justify: rich::Justify,
    pub(crate) overflow: Option<rich::Overflow>,
    pub(crate) no_wrap: Option<bool>,
    pub(crate) indent_guides: bool,
    pub(crate) expand_all: bool,
    pub(crate) margin: usize,
    pub(crate) insert_line: bool,
}

impl Layout {
    /// The text `__rich_console__` yields (after an empty line when
    /// `insert_line` applies, the returned flag).
    fn text(
        &self,
        py: Python<'_>,
        highlight: &Highlight,
        options: &CoreOptions,
        ascii_only: bool,
    ) -> PyResult<(bool, CoreText)> {
        let width = options.max_width as isize - self.margin as isize;
        let pretty = self.node.render(width, self.indent_size, self.expand_all);
        let mut text = from_ansi(&pretty, "pretty");
        text.set_justify(if self.justify == rich::Justify::Default {
            options.justify
        } else {
            self.justify
        });
        text.set_overflow(self.overflow.or(options.overflow));
        text.set_no_wrap(self.no_wrap.or(options.no_wrap));
        let mut text = if text.plain().is_empty() {
            CoreText::styled(
                format!("{}.__repr__ returned empty string", self.type_repr),
                StyleType::Name("dim italic".to_string()),
            )
        } else {
            highlight.apply(py, text)?
        };
        if self.indent_guides && !ascii_only {
            text = with_indent_guides(
                &text,
                self.indent_size,
                StyleType::Name("repr.indent".to_string()),
            );
        }
        Ok((self.insert_line && text.plain().contains('\n'), text))
    }

    fn measure(&self, options: &CoreOptions) -> CoreMeasurement {
        let pretty = self.node.render(
            options.max_width as isize,
            self.indent_size,
            self.expand_all,
        );
        let width = pretty.lines().map(cell_len).max().unwrap_or(0);
        CoreMeasurement::new(width, width)
    }
}

impl Layout {
    /// `Pretty(obj)`'s defaults, for a traversed `node`.
    pub(crate) fn new(node: Node, type_repr: String) -> Layout {
        Layout {
            node,
            type_repr,
            indent_size: 4,
            justify: rich::Justify::Default,
            overflow: None,
            no_wrap: Some(false),
            indent_guides: false,
            expand_all: false,
            margin: 0,
            insert_line: false,
        }
    }
}

/// A `Pretty` with the repr highlighter, shareable as a table cell. `map`
/// renames the highlighter's styles (see `layout::restyle`).
pub(crate) fn shared_repr(
    layout: Layout,
    map: &'static [(&'static str, &'static str)],
) -> std::sync::Arc<dyn Renderable + Send + Sync> {
    std::sync::Arc::new(NativePretty {
        layout,
        highlight: Highlight::Repr,
        map,
    })
}

/// A `Pretty` whose highlighter is Rust's: rendering never calls Python.
struct NativePretty {
    layout: Layout,
    highlight: Highlight,
    map: &'static [(&'static str, &'static str)],
}

impl Renderable for NativePretty {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        // The highlight is Repr or Null, which never fails or calls Python.
        let rendered = Python::attach(|py| {
            self.layout
                .text(py, &self.highlight, options, console.ascii_only())
        });
        let Ok((blank, mut text)) = rendered else {
            return Vec::new();
        };
        if !self.map.is_empty() {
            text = super::layout::restyle(&text, self.map);
        }
        let mut segments = Vec::new();
        if blank {
            segments.push(CoreSegment::line());
        }
        segments.extend(text.rich_render(console, options));
        segments
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.layout.measure(options)
    }
}

/// A `Pretty` whose highlighter is Python code: rendered through Rich's
/// protocol, so an exception it raises reaches the caller.
#[pyclass(module = "rs_rich.pretty", frozen)]
struct PrettyLayout {
    layout: Layout,
    highlight: Highlight,
}

#[pymethods]
impl PrettyLayout {
    fn __rich_console__(
        &self,
        py: Python<'_>,
        console: &Bound<'_, PyAny>,
        options: PyRef<'_, crate::protocol::ConsoleOptions>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let core = options.to_core()?;
        let ascii_only = !options.base().encoding.to_lowercase().starts_with("utf");
        let _ = console;
        let (blank, text) = self.layout.text(py, &self.highlight, &core, ascii_only)?;
        let mut items = Vec::new();
        if blank {
            items.push(PyString::new(py, "").into_any().unbind());
        }
        items.push(super::highlighter::new_text(py, text)?.into_any().unbind());
        Ok(items)
    }

    fn __rich_measure__(
        &self,
        _console: &Bound<'_, PyAny>,
        options: PyRef<'_, crate::protocol::ConsoleOptions>,
    ) -> PyResult<crate::protocol::Measurement> {
        let core = options.to_core()?;
        Ok(crate::protocol::Measurement::from_core(
            self.layout.measure(&core),
        ))
    }
}

/// The renderable for a layout: native unless the highlighter is Python.
pub(crate) fn renderable_for(
    py: Python<'_>,
    layout: Layout,
    highlight: Highlight,
) -> PyResult<Box<dyn Renderable>> {
    if highlight.is_python() {
        let object = Py::new(py, PrettyLayout { layout, highlight })?;
        Ok(Box::new(PyRenderable::new(object.into_any())))
    } else {
        Ok(Box::new(NativePretty {
            layout,
            highlight,
            map: &[],
        }))
    }
}

/// `rich.pretty.Pretty`: a renderable that pretty-prints any object.
#[pyclass(name = "Pretty", module = "rs_rich.pretty")]
pub(crate) struct Pretty {
    object: Py<PyAny>,
    highlighter: Option<Py<PyAny>>,
    #[pyo3(get, set)]
    indent_size: usize,
    #[pyo3(get, set)]
    justify: Option<String>,
    #[pyo3(get, set)]
    overflow: Option<String>,
    #[pyo3(get, set)]
    no_wrap: Option<bool>,
    #[pyo3(get, set)]
    indent_guides: bool,
    #[pyo3(get, set)]
    max_length: Option<usize>,
    #[pyo3(get, set)]
    max_string: Option<usize>,
    #[pyo3(get, set)]
    max_depth: Option<usize>,
    #[pyo3(get, set)]
    expand_all: bool,
    #[pyo3(get, set)]
    margin: usize,
    #[pyo3(get, set)]
    insert_line: bool,
}

impl Pretty {
    pub(crate) fn limits(&self) -> Limits {
        Limits {
            max_length: self.max_length,
            max_string: self.max_string,
            max_depth: self.max_depth,
        }
    }
}

/// The `repr` of an object's type (for the empty-repr message).
pub(crate) fn type_repr(object: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(object.get_type().repr()?.to_string())
}

impl AsRenderable for Pretty {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let object = self.object.bind(py);
        let node = traverse(object, self.limits())?;
        node.check_indent(self.indent_size)?;
        let layout = Layout {
            node,
            type_repr: type_repr(object)?,
            indent_size: self.indent_size,
            justify: convert::justify(self.justify.as_deref())?,
            overflow: self
                .overflow
                .as_deref()
                .map(convert::overflow)
                .transpose()?,
            no_wrap: self.no_wrap,
            indent_guides: self.indent_guides,
            expand_all: self.expand_all,
            margin: self.margin,
            insert_line: self.insert_line,
        };
        let highlight = Highlight::from_arg(self.highlighter.as_ref().map(|h| h.bind(py)))?;
        renderable_for(py, layout, highlight)
    }
}

#[pymethods]
impl Pretty {
    #[new]
    #[pyo3(signature = (
        _object, highlighter=None, *, indent_size=4, justify=None, overflow=None, no_wrap=Some(false),
        indent_guides=false, max_length=None, max_string=None, max_depth=None, expand_all=false,
        margin=0, insert_line=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        _object: Py<PyAny>,
        highlighter: Option<Py<PyAny>>,
        indent_size: usize,
        justify: Option<String>,
        overflow: Option<String>,
        no_wrap: Option<bool>,
        indent_guides: bool,
        max_length: Option<usize>,
        max_string: Option<usize>,
        max_depth: Option<usize>,
        expand_all: bool,
        margin: usize,
        insert_line: bool,
    ) -> PyResult<Self> {
        convert::justify(justify.as_deref())?;
        overflow.as_deref().map(convert::overflow).transpose()?;
        Ok(Pretty {
            object: _object,
            highlighter,
            indent_size,
            justify,
            overflow,
            no_wrap,
            indent_guides,
            max_length,
            max_string,
            max_depth,
            expand_all,
            margin,
            insert_line,
        })
    }

    #[getter]
    fn _object(&self, py: Python<'_>) -> Py<PyAny> {
        self.object.clone_ref(py)
    }

    #[getter]
    fn highlighter(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.highlighter {
            Some(highlighter) => Ok(highlighter.clone_ref(py)),
            None => Ok(py
                .get_type::<super::highlighter::ReprHighlighter>()
                .call0()?
                .unbind()),
        }
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.object)?;
        if let Some(highlighter) = &self.highlighter {
            visit.call(highlighter)?;
        }
        Ok(())
    }
}

/// `rich.pretty.Node`: one node of the tree `traverse` builds.
#[pyclass(name = "Node", module = "rs_rich.pretty", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyNode {
    pub(crate) inner: Node,
}

#[pymethods]
impl PyNode {
    #[new]
    #[pyo3(signature = (
        key_repr=String::new(), value_repr=String::new(), open_brace=String::new(),
        close_brace=String::new(), empty=String::new(), last=false, is_tuple=false,
        is_namedtuple=false, children=None, key_separator=": ".to_string(),
        separator=", ".to_string()
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        key_repr: String,
        value_repr: String,
        open_brace: String,
        close_brace: String,
        empty: String,
        last: bool,
        is_tuple: bool,
        is_namedtuple: bool,
        children: Option<Vec<PyRef<'_, PyNode>>>,
        key_separator: String,
        separator: String,
    ) -> Self {
        PyNode {
            inner: Node {
                key_repr,
                value_repr,
                open_brace,
                close_brace,
                empty,
                last,
                is_tuple,
                is_namedtuple,
                children: children
                    .map(|c| c.iter().map(|n| n.inner.clone()).collect())
                    .into(),
                key_separator,
                separator,
            },
        }
    }

    #[getter]
    fn key_repr(&self) -> &str {
        &self.inner.key_repr
    }
    #[setter]
    fn set_key_repr(&mut self, value: String) {
        self.inner.key_repr = value;
    }
    #[getter]
    fn value_repr(&self) -> &str {
        &self.inner.value_repr
    }
    #[setter]
    fn set_value_repr(&mut self, value: String) {
        self.inner.value_repr = value;
    }
    #[getter]
    fn open_brace(&self) -> &str {
        &self.inner.open_brace
    }
    #[setter]
    fn set_open_brace(&mut self, value: String) {
        self.inner.open_brace = value;
    }
    #[getter]
    fn close_brace(&self) -> &str {
        &self.inner.close_brace
    }
    #[setter]
    fn set_close_brace(&mut self, value: String) {
        self.inner.close_brace = value;
    }
    #[getter]
    fn empty(&self) -> &str {
        &self.inner.empty
    }
    #[setter]
    fn set_empty(&mut self, value: String) {
        self.inner.empty = value;
    }
    #[getter]
    fn last(&self) -> bool {
        self.inner.last
    }
    #[setter]
    fn set_last(&mut self, value: bool) {
        self.inner.last = value;
    }
    #[getter]
    fn is_tuple(&self) -> bool {
        self.inner.is_tuple
    }
    #[setter]
    fn set_is_tuple(&mut self, value: bool) {
        self.inner.is_tuple = value;
    }
    #[getter]
    fn is_namedtuple(&self) -> bool {
        self.inner.is_namedtuple
    }
    #[setter]
    fn set_is_namedtuple(&mut self, value: bool) {
        self.inner.is_namedtuple = value;
    }
    #[getter]
    fn key_separator(&self) -> &str {
        &self.inner.key_separator
    }
    #[setter]
    fn set_key_separator(&mut self, value: String) {
        self.inner.key_separator = value;
    }
    #[getter]
    fn separator(&self) -> &str {
        &self.inner.separator
    }
    #[setter]
    fn set_separator(&mut self, value: String) {
        self.inner.separator = value;
    }

    /// The children, as copies (assign a list to change them).
    #[getter]
    fn children(&self) -> Option<Vec<PyNode>> {
        self.inner.children.as_ref().map(|children| {
            children
                .iter()
                .map(|child| PyNode {
                    inner: child.clone(),
                })
                .collect()
        })
    }
    #[setter]
    fn set_children(&mut self, value: Option<Vec<PyRef<'_, PyNode>>>) {
        self.inner.children = value
            .map(|c| c.iter().map(|n| n.inner.clone()).collect())
            .into();
    }

    fn iter_tokens<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        PyList::new(py, self.inner.token_list())?
            .try_iter()
            .map(Bound::into_any)
    }

    fn check_length(&self, start_length: isize, max_length: isize) -> bool {
        self.inner.check_length(start_length, max_length)
    }

    #[pyo3(signature = (max_width=80, indent_size=4, expand_all=false))]
    fn render(&self, max_width: isize, indent_size: usize, expand_all: bool) -> PyResult<String> {
        self.inner.check_indent(indent_size)?;
        Ok(self.inner.render(max_width, indent_size, expand_all))
    }

    fn __str__(&self) -> String {
        self.inner.to_repr()
    }

    fn __repr__(&self) -> String {
        let node = &self.inner;
        format!(
            "Node(key_repr={:?}, value_repr={:?}, open_brace={:?}, close_brace={:?}, empty={:?}, \
             last={}, is_tuple={}, is_namedtuple={}, children=..., key_separator={:?}, separator={:?})",
            node.key_repr,
            node.value_repr,
            node.open_brace,
            node.close_brace,
            node.empty,
            py_bool(node.last),
            py_bool(node.is_tuple),
            py_bool(node.is_namedtuple),
            node.key_separator,
            node.separator
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, PyNode>>()
            .is_ok_and(|other| other.inner.same(&self.inner))
    }
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Python frames upstream's `traverse` takes before `_traverse(obj, depth=0)`
/// runs out of them (`pretty_repr` takes one more).
const TRAVERSE_FRAMES: usize = 7;

/// `rich.pretty.traverse(_object, max_length=None, max_string=None, max_depth=None)`.
#[pyfunction(name = "traverse")]
#[pyo3(signature = (_object, max_length=None, max_string=None, max_depth=None))]
fn traverse_py(
    _object: &Bound<'_, PyAny>,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<PyNode> {
    Ok(PyNode {
        inner: traverse_with(
            _object,
            Limits {
                max_length,
                max_string,
                max_depth,
            },
            TRAVERSE_FRAMES,
        )?,
    })
}

/// `rich.pretty.pretty_repr`: a repr expanded onto lines to fit `max_width`.
#[pyfunction]
#[pyo3(signature = (
    _object, *, max_width=80, indent_size=4, max_length=None, max_string=None, max_depth=None,
    expand_all=false
))]
fn pretty_repr(
    _object: &Bound<'_, PyAny>,
    max_width: isize,
    indent_size: usize,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<String> {
    let node = match _object.extract::<PyRef<'_, PyNode>>() {
        Ok(node) => node.inner.clone(),
        Err(_) => traverse_with(
            _object,
            Limits {
                max_length,
                max_string,
                max_depth,
            },
            TRAVERSE_FRAMES + 1,
        )?,
    };
    node.check_indent(indent_size)?;
    Ok(node.render(max_width, indent_size, expand_all))
}

/// The console an optional `console=` argument names (`rs_rich.get_console()`
/// when it is `None`).
pub(crate) fn console_or_global<'py>(
    py: Python<'py>,
    console: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    match console.filter(|c| !c.is_none()) {
        Some(console) => Ok(console.clone()),
        None => py.import("rs_rich")?.call_method0("get_console"),
    }
}

/// `rich.pretty.pprint`: pretty-print an object, with indent guides.
#[pyfunction]
#[pyo3(signature = (
    _object, *, console=None, indent_guides=true, max_length=None, max_string=None,
    max_depth=None, expand_all=false
))]
#[allow(clippy::too_many_arguments)]
fn pprint(
    py: Python<'_>,
    _object: Py<PyAny>,
    console: Option<&Bound<'_, PyAny>>,
    indent_guides: bool,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<()> {
    let console = console_or_global(py, console)?;
    let pretty = Pretty {
        object: _object,
        highlighter: None,
        indent_size: 4,
        justify: None,
        overflow: Some("ignore".to_string()),
        no_wrap: Some(false),
        indent_guides,
        max_length,
        max_string,
        max_depth,
        expand_all,
        margin: 0,
        insert_line: false,
    };
    let kwargs = PyDict::new(py);
    kwargs.set_item("soft_wrap", true)?;
    console.call_method("print", (Py::new(py, pretty)?,), Some(&kwargs))?;
    Ok(())
}

/// `isinstance(value, RichRenderable)`: one of the bindings' renderables, or
/// an object with `__rich__` or `__rich_console__`.
fn is_rich_renderable(value: &Bound<'_, PyAny>) -> bool {
    renderable::is_registered(value)
        || value.hasattr("__rich__").unwrap_or(false)
        || value.hasattr("__rich_console__").unwrap_or(false)
}

/// `rich.pretty.install`: pretty-print values in the REPL (`sys.displayhook`).
#[pyfunction]
#[pyo3(signature = (
    console=None, overflow="ignore".to_string(), crop=false, indent_guides=false, max_length=None,
    max_string=None, max_depth=None, expand_all=false
))]
#[allow(clippy::too_many_arguments)]
fn pretty_install(
    py: Python<'_>,
    console: Option<&Bound<'_, PyAny>>,
    overflow: Option<String>,
    crop: bool,
    indent_guides: bool,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<()> {
    overflow.as_deref().map(convert::overflow).transpose()?;
    let console = console_or_global(py, console)?.unbind();
    let hook = pyo3::types::PyCFunction::new_closure(
        py,
        Some(c"display_hook"),
        Some(c"Replacement sys.displayhook which prettifies objects with rs_rich."),
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let py = args.py();
            let value = args.get_item(0)?;
            if value.is_none() {
                return Ok(());
            }
            let builtins = py.import("builtins")?;
            builtins.setattr("_", py.None())?;
            let printed: Py<PyAny> = if is_rich_renderable(&value) {
                value.clone().unbind()
            } else {
                Py::new(
                    py,
                    Pretty {
                        object: value.clone().unbind(),
                        highlighter: None,
                        indent_size: 4,
                        justify: None,
                        overflow: overflow.clone(),
                        no_wrap: Some(false),
                        indent_guides,
                        max_length,
                        max_string,
                        max_depth,
                        expand_all,
                        margin: 0,
                        insert_line: false,
                    },
                )?
                .into_any()
            };
            let kwargs = PyDict::new(py);
            kwargs.set_item("crop", crop)?;
            console
                .bind(py)
                .call_method("print", (printed,), Some(&kwargs))?;
            builtins.setattr("_", value)?;
            Ok(())
        },
    )?;
    py.import("sys")?.setattr("displayhook", hook)?;
    Ok(())
}

/// What `Console.print` renders for an expandable object:
/// `Pretty(obj, highlighter=...)` with the repr or the null highlighter.
pub(crate) fn for_print(
    value: &Bound<'_, PyAny>,
    highlight: bool,
) -> PyResult<Box<dyn Renderable>> {
    let py = value.py();
    let layout = Layout {
        node: traverse(value, Limits::default())?,
        type_repr: type_repr(value)?,
        indent_size: 4,
        justify: rich::Justify::Default,
        overflow: None,
        no_wrap: Some(false),
        indent_guides: false,
        expand_all: false,
        margin: 0,
        insert_line: false,
    };
    let highlight = if highlight {
        Highlight::Repr
    } else {
        Highlight::Null
    };
    renderable_for(py, layout, highlight)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Pretty>(m)?;
    m.add_class::<PyNode>()?;
    m.add_function(wrap_pyfunction!(traverse_py, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_repr, m)?)?;
    m.add_function(wrap_pyfunction!(pprint, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_install, m)?)?;
    Ok(())
}
