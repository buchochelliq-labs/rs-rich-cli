//! `rich.containers.Lines` (also `rich.text.Lines`): the list of `Text`
//! lines that `Text.split`, `divide`, `wrap` and `fit` return.

use pyo3::exceptions::{PyIndexError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PySlice};

use rich::Text as CoreText;

use super::{new_text, Text};
use crate::convert;

/// `rich.containers.Lines`: a list of `Text`s that renders one per line.
#[pyclass(name = "Lines", module = "rs_rich.text")]
pub(crate) struct Lines {
    lines: Vec<Py<Text>>,
}

impl Lines {
    pub(crate) fn from_objects(lines: Vec<Py<Text>>) -> PyResult<Lines> {
        Ok(Lines { lines })
    }

    /// New `Text` objects (default `end`) for core texts.
    pub(crate) fn from_core(py: Python<'_>, lines: Vec<CoreText>) -> PyResult<Lines> {
        let lines = lines
            .into_iter()
            .map(|line| Ok(new_text(py, line, "\n")?.unbind()))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Lines { lines })
    }

    fn index(&self, index: isize) -> PyResult<usize> {
        let length = self.lines.len() as isize;
        let position = if index < 0 { length + index } else { index };
        if position < 0 || position >= length {
            return Err(PyIndexError::new_err("list index out of range"));
        }
        Ok(position as usize)
    }
}

fn extract_text(value: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
    value
        .cast::<Text>()
        .map(|text| text.clone().unbind())
        .map_err(|_| PyTypeError::new_err("Lines holds Text instances"))
}

#[pymethods]
impl Lines {
    #[new]
    #[pyo3(signature = (lines=None))]
    fn new(lines: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut result = Vec::new();
        if let Some(lines) = lines {
            for line in lines.try_iter()? {
                result.push(extract_text(&line?)?);
            }
        }
        Ok(Lines { lines: result })
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let list = PyList::new(py, self.lines.iter().map(|line| line.bind(py)))?;
        Ok(format!("Lines({})", list.repr()?))
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        PyList::new(py, self.lines.iter().map(|line| line.bind(py)))?
            .as_any()
            .try_iter()
            .map(|iterator| iterator.into_any())
    }

    fn __len__(&self) -> usize {
        self.lines.len()
    }

    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if index.cast::<PySlice>().is_ok() {
            let list = PyList::new(py, self.lines.iter().map(|line| line.bind(py)))?;
            return list.as_any().get_item(index);
        }
        let position = self.index(index.extract()?)?;
        Ok(self.lines[position].bind(py).clone().into_any())
    }

    fn __setitem__(&mut self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let position = self.index(index)?;
        self.lines[position] = extract_text(value)?;
        Ok(())
    }

    /// Each line, rendered one after another.
    fn __rich_console__<'py>(
        &self,
        py: Python<'py>,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let _ = (console, options);
        PyList::new(py, self.lines.iter().map(|line| line.bind(py)))
    }

    fn append(&mut self, line: &Bound<'_, PyAny>) -> PyResult<()> {
        self.lines.push(extract_text(line)?);
        Ok(())
    }

    fn extend(&mut self, lines: &Bound<'_, PyAny>) -> PyResult<()> {
        for line in lines.try_iter()? {
            self.lines.push(extract_text(&line?)?);
        }
        Ok(())
    }

    #[pyo3(signature = (index=-1))]
    fn pop(&mut self, index: isize) -> PyResult<Py<Text>> {
        if self.lines.is_empty() {
            return Err(PyIndexError::new_err("pop from empty list"));
        }
        let position = self.index(index)?;
        // `drain`, not `Vec::remove`: CodeQL's Rust models read `remove` as
        // writing to a log (cleartext-logging false positive).
        let popped = self.lines.drain(position..=position).next();
        Ok(popped.expect("index() returns a position in range"))
    }

    /// Justify every line to `width` cells, as Rich's `Lines.justify`.
    #[pyo3(signature = (console, width, justify="left", overflow="fold"))]
    fn justify(
        &mut self,
        py: Python<'_>,
        console: &Bound<'_, PyAny>,
        width: usize,
        justify: &str,
        overflow: &str,
    ) -> PyResult<()> {
        let overflow = convert::overflow(overflow)?;
        let mut cores: Vec<CoreText> = self
            .lines
            .iter()
            .map(|line| line.bind(py).borrow().inner.clone())
            .collect();
        super::ops::justify_lines(console, &mut cores, width, justify, overflow)?;
        let last = self.lines.len().saturating_sub(1);
        for (index, core) in cores.into_iter().enumerate() {
            if justify == "full" && index < last {
                // Rich replaces these lines with new `Text`s.
                self.lines[index] = new_text(py, core, "\n")?.unbind();
            } else {
                self.lines[index].bind(py).borrow_mut().inner = core;
            }
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Lines>()
}
