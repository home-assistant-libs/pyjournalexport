//! Python bindings for the journal export parser.

pub mod parser;

use parser::{Entry, ParseError, Parser};
use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

create_exception!(
    pyjournalexport,
    MalformedBinaryEntryError,
    PyValueError,
    "A binary field is malformed or the stream ended inside one."
);

impl From<ParseError> for PyErr {
    fn from(err: ParseError) -> Self {
        MalformedBinaryEntryError::new_err(err.to_string())
    }
}

/// Incremental parser for the systemd journal export format.
///
/// Feed it the raw stream in any chunking and it returns the completed
/// entries as dicts holding only the requested fields.
#[pyclass(name = "JournalExportParser", module = "pyjournalexport")]
struct PyParser {
    parser: Parser,
    names: Vec<Py<PyString>>,
}

impl PyParser {
    fn to_dicts<'py>(&self, py: Python<'py>, entries: Vec<Entry>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for entry in entries {
            let dict = PyDict::new(py);
            for (index, value) in entry {
                dict.set_item(self.names[index].bind(py), value)?;
            }
            list.append(dict)?;
        }
        Ok(list)
    }
}

#[pymethods]
impl PyParser {
    #[new]
    fn new(py: Python<'_>, fields: Vec<String>) -> Self {
        Self {
            names: fields
                .iter()
                .map(|f| PyString::new(py, f).unbind())
                .collect(),
            parser: Parser::new(&fields),
        }
    }

    /// Parse data and return the entries completed by it.
    fn feed<'py>(&mut self, py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyList>> {
        let entries = self.parser.feed(data)?;
        self.to_dicts(py, entries)
    }

    /// Return a trailing entry that was cut off at the end of the stream.
    fn close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let entries = self.parser.close()?;
        self.to_dicts(py, entries)
    }
}

#[pymodule]
fn _parser(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyParser>()?;
    m.add(
        "MalformedBinaryEntryError",
        py.get_type::<MalformedBinaryEntryError>(),
    )?;
    Ok(())
}
