use std::borrow::Cow;
use std::fmt::Write;

// Use `::monty` to refer to the external crate (not the pymodule)
use ::monty::{
    LimitedTracker, MontyObject, NoLimitTracker, PrintWriter, ResourceTracker, RunProgress, RunSnapshot, Snapshot,
    StdPrint,
};
use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyTypeError};
use pyo3::types::{PyDict, PyList, PyTuple};
use pyo3::{prelude::*, IntoPyObjectExt};

use crate::convert::{monty_to_py, py_to_monty};
use crate::exceptions::monty_exc_to_py;
use crate::external::ExternalFunctionRegistry;
use crate::limits::PyResourceLimits;

/// A sandboxed Python interpreter instance.
///
/// Parses and compiles Python code on initialization, then can be run
/// multiple times with different input values. This separates the parsing
/// cost from execution, making repeated runs more efficient.
#[pyclass(name = "Monty")]
#[derive(Debug)]
pub struct PyMonty {
    /// The compiled code snapshot, ready to execute.
    runner: RunSnapshot,
    /// The artificial name of the python code "file"
    script_name: String,
    /// Names of input variables expected by the code.
    input_names: Vec<String>,
    /// Names of external functions the code can call.
    external_function_names: Vec<String>,
}

#[pymethods]
impl PyMonty {
    /// Creates a new Monty interpreter by parsing the given code.
    ///
    /// # Arguments
    /// * `code` - Python code to execute
    /// * `inputs` - List of input variable names available in the code
    /// * `external_functions` - List of external function names the code can call
    ///
    /// # Raises
    /// `SyntaxError` if the code cannot be parsed
    #[new]
    #[pyo3(signature = (code, *, script_name="main.py", inputs=None, external_functions=None))]
    fn new(
        code: String,
        script_name: &str,
        inputs: Option<&Bound<'_, PyList>>,
        external_functions: Option<&Bound<'_, PyList>>,
    ) -> PyResult<Self> {
        let input_names = list_str(inputs, "inputs")?;
        let external_function_names = list_str(external_functions, "external_functions")?;

        // Create the snapshot (parses the code)
        let runner = RunSnapshot::new(code, script_name, input_names.clone(), external_function_names.clone())
            .map_err(monty_exc_to_py)?;

        Ok(Self {
            runner,
            script_name: script_name.to_string(),
            input_names,
            external_function_names,
        })
    }

    /// Executes the code and returns the result.
    ///
    /// # Arguments
    /// * `inputs` - Dict of input variable values (must match names from `__init__`)
    /// * `limits` - Optional `ResourceLimits` configuration
    /// * `external_functions` - Dict of external function callbacks (must match names from `__init__`)
    ///
    /// # Returns
    /// The result of the last expression in the code
    ///
    /// # Raises
    /// Various Python exceptions matching what the code would raise
    #[pyo3(signature = (*, inputs=None, limits=None, external_functions=None, print_callback=None))]
    fn run(
        &self,
        py: Python<'_>,
        inputs: Option<&Bound<'_, PyDict>>,
        limits: Option<&PyResourceLimits>,
        external_functions: Option<&Bound<'_, PyDict>>,
        print_callback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        // Extract input values in the order they were declared
        let input_values = self.extract_input_values(inputs)?;

        /// if there are no external functions, run the code without a snapshotting for better performance
        macro_rules! run_code {
            ($resource_tracker:expr, $print_output:expr) => {{
                if self.external_function_names.is_empty() {
                    match self
                        .runner
                        .run_no_snapshot(input_values, $resource_tracker, &mut $print_output)
                    {
                        Ok(v) => monty_to_py(py, &v),
                        Err(err) => Err(monty_exc_to_py(err)),
                    }
                } else {
                    // Clone the snapshot since run_snapshot methods consume it - allows reuse of the parsed code
                    let progress = self
                        .runner
                        .clone()
                        .run_snapshot(input_values, $resource_tracker, &mut $print_output)
                        .map_err(monty_exc_to_py)?;
                    execute_progress(py, progress, external_functions, &mut $print_output)
                }
            }};
        }

        // separate code paths due to generics
        match (limits, print_callback) {
            (Some(limits), Some(callback)) => {
                run_code!(
                    LimitedTracker::new(limits.to_monty_limits()),
                    CallbackStringPrint(callback)
                )
            }
            (Some(limits), None) => {
                run_code!(LimitedTracker::new(limits.to_monty_limits()), StdPrint)
            }
            (None, Some(callback)) => {
                run_code!(NoLimitTracker::default(), CallbackStringPrint(callback))
            }
            (None, None) => {
                run_code!(NoLimitTracker::default(), StdPrint)
            }
        }
    }

    #[pyo3(signature = (*, inputs=None, limits=None, print_callback=None))]
    fn start<'py>(
        &self,
        py: Python<'py>,
        inputs: Option<&Bound<'py, PyDict>>,
        limits: Option<&PyResourceLimits>,
        print_callback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Extract input values in the order they were declared
        let input_values = self.extract_input_values(inputs)?;

        /// if there are no external functions, run the code without a snapshotting for better performance
        macro_rules! start {
            ($resource_tracker:expr, $print_output:expr) => {
                // Clone the snapshot since run_snapshot methods consume it - allows reuse of the parsed code
                self.runner
                    .clone()
                    .run_snapshot(input_values, $resource_tracker, &mut $print_output)
                    .map_err(monty_exc_to_py)?
            };
        }

        // separate code paths due to generics
        let progress = match (limits, print_callback) {
            (Some(limits), Some(callback)) => EitherProgress::Limited(start!(
                LimitedTracker::new(limits.to_monty_limits()),
                CallbackStringPrint(callback)
            )),
            (Some(limits), None) => {
                EitherProgress::Limited(start!(LimitedTracker::new(limits.to_monty_limits()), StdPrint))
            }
            (None, Some(callback)) => {
                EitherProgress::NoLimit(start!(NoLimitTracker::default(), CallbackStringPrint(callback)))
            }
            (None, None) => EitherProgress::NoLimit(start!(NoLimitTracker::default(), StdPrint)),
        };
        progress.progress_or_complete(py, self.script_name.clone(), print_callback.map(|c| c.clone().unbind()))
    }

    fn __repr__(&self) -> String {
        let lines = self.runner.code().lines().count();
        let mut s = format!(
            "Monty(<{} line{} of code>, script_name='{}'",
            lines,
            if lines == 1 { "" } else { "s" },
            self.script_name
        );
        if !self.input_names.is_empty() {
            write!(s, ", inputs={:?}", self.input_names).unwrap();
        }
        if !self.external_function_names.is_empty() {
            write!(s, ", external_functions={:?}", self.external_function_names).unwrap();
        }
        s.push(')');
        s
    }
}

impl PyMonty {
    /// Extracts input values from the dict in the order they were declared.
    ///
    /// Validates that all required inputs are provided and no extra inputs are given.
    fn extract_input_values(&self, inputs: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<::monty::MontyObject>> {
        if self.input_names.is_empty() {
            if inputs.is_some() {
                return Err(PyTypeError::new_err(
                    "No input variables declared but inputs dict was provided",
                ));
            }
            return Ok(vec![]);
        }

        let Some(inputs) = inputs else {
            return Err(PyTypeError::new_err(format!(
                "Missing required inputs: {:?}",
                self.input_names
            )));
        };

        // Extract values in declaration order
        self.input_names
            .iter()
            .map(|name| {
                let value = inputs
                    .get_item(name)?
                    .ok_or_else(|| PyKeyError::new_err(format!("Missing required input: '{name}'")))?;
                py_to_monty(&value)
            })
            .collect::<PyResult<_>>()
    }
}

/// pyclass doesn't support generic types, hence hard coding the generics
#[derive(Debug)]
enum EitherProgress {
    NoLimit(RunProgress<NoLimitTracker>),
    Limited(RunProgress<LimitedTracker>),
}

impl EitherProgress {
    fn progress_or_complete(
        self,
        py: Python<'_>,
        script_name: String,
        print_callback: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'_, PyAny>> {
        let (function_name, args, kwargs, snapshot) = match self {
            EitherProgress::NoLimit(p) => match p {
                RunProgress::Complete(result) => return PyMontyComplete::create(py, &result),
                RunProgress::FunctionCall {
                    function_name,
                    args,
                    kwargs,
                    state,
                } => (function_name, args, kwargs, EitherSnapshot::NoLimit(state)),
            },
            EitherProgress::Limited(p) => match p {
                RunProgress::Complete(result) => return PyMontyComplete::create(py, &result),
                RunProgress::FunctionCall {
                    function_name,
                    args,
                    kwargs,
                    state,
                } => (function_name, args, kwargs, EitherSnapshot::Limited(state)),
            },
        };

        let items: PyResult<Vec<Py<PyAny>>> = args.iter().map(|item| monty_to_py(py, item)).collect();

        let dict = PyDict::new(py);
        for (k, v) in &kwargs {
            dict.set_item(monty_to_py(py, k)?, monty_to_py(py, v)?)?;
        }

        let slf = PyMontyProgress {
            snapshot,
            print_callback: print_callback.map(|callback| callback.clone_ref(py)),
            script_name,
            function_name,
            args: PyTuple::new(py, items?)?.unbind(),
            kwargs: dict.unbind(),
        };
        slf.into_bound_py_any(py)
    }
}

#[derive(Debug)]
enum EitherSnapshot {
    NoLimit(Snapshot<NoLimitTracker>),
    Limited(Snapshot<LimitedTracker>),
    /// Done is used when taking the snapshot to run it
    /// should only be done after execution is complete
    Done,
}

#[pyclass(name = "MontyProgress")]
#[derive(Debug)]
pub struct PyMontyProgress {
    snapshot: EitherSnapshot,
    print_callback: Option<Py<PyAny>>,

    /// Name of the script being executed
    #[pyo3(get)]
    pub script_name: String,

    /// The name of the function being called.
    #[pyo3(get)]
    pub function_name: String,
    /// The positional arguments passed to the function.
    #[pyo3(get)]
    pub args: Py<PyTuple>,
    /// The keyword arguments passed to the function (key, value pairs).
    #[pyo3(get)]
    pub kwargs: Py<PyDict>,
}

#[pymethods]
impl PyMontyProgress {
    pub fn resume<'py>(&mut self, py: Python<'py>, return_value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let monty_return_value = py_to_monty(return_value)?;
        let snapshot = std::mem::replace(&mut self.snapshot, EitherSnapshot::Done);
        let progress = match snapshot {
            EitherSnapshot::NoLimit(snapshot) => {
                let result = if let Some(print_callback) = &self.print_callback {
                    snapshot.run(monty_return_value, &mut CallbackStringPrint(print_callback.bind(py)))
                } else {
                    snapshot.run(monty_return_value, &mut StdPrint)
                };
                EitherProgress::NoLimit(result.map_err(monty_exc_to_py)?)
            }
            EitherSnapshot::Limited(snapshot) => {
                let result = if let Some(print_callback) = &self.print_callback {
                    snapshot.run(monty_return_value, &mut CallbackStringPrint(print_callback.bind(py)))
                } else {
                    snapshot.run(monty_return_value, &mut StdPrint)
                };
                EitherProgress::Limited(result.map_err(monty_exc_to_py)?)
            }
            EitherSnapshot::Done => return Err(PyRuntimeError::new_err("Progress already resumed")),
        };

        progress.progress_or_complete(py, self.script_name.clone(), self.print_callback.take())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "MontyProgress(script_name='{}', function_name='{}', args={}, kwargs={})",
            self.script_name,
            self.function_name,
            self.args.bind(py).repr()?,
            self.kwargs.bind(py).repr()?
        ))
    }
}

#[pyclass(name = "MontyComplete")]
pub struct PyMontyComplete {
    #[pyo3(get)]
    pub output: Py<PyAny>,
    // TODO we might want to add stats on execution here like time, allocations, etc.
}

impl PyMontyComplete {
    fn create<'py>(py: Python<'py>, output: &MontyObject) -> PyResult<Bound<'py, PyAny>> {
        let output = monty_to_py(py, output)?;
        let slf = Self { output };
        slf.into_bound_py_any(py)
    }
}

#[pymethods]
impl PyMontyComplete {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("MontyComplete(output={})", self.output.bind(py).repr()?))
    }
}

/// Executes the `RunProgress` loop, handling external function calls.
///
/// Uses a generic type to handle both `NoLimitTracker` and `LimitedTracker`.
fn execute_progress(
    py: Python<'_>,
    mut progress: RunProgress<impl ResourceTracker>,
    external_functions: Option<&Bound<'_, PyDict>>,
    print_output: &mut impl PrintWriter,
) -> PyResult<Py<PyAny>> {
    loop {
        match progress {
            RunProgress::Complete(result) => {
                return monty_to_py(py, &result);
            }
            RunProgress::FunctionCall {
                function_name,
                args,
                kwargs,
                state,
            } => {
                let registry = external_functions
                    .map(|d| ExternalFunctionRegistry::new(py, d))
                    .ok_or_else(|| {
                        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                            "External function '{function_name}' called but no external_functions provided"
                        ))
                    })?;

                let return_value = registry.call(&function_name, &args, &kwargs);

                progress = state.run(return_value, print_output).map_err(monty_exc_to_py)?;
            }
        }
    }
}

fn list_str(arg: Option<&Bound<'_, PyList>>, name: &str) -> PyResult<Vec<String>> {
    if let Some(names) = arg {
        names
            .iter()
            .map(|item| item.extract::<String>())
            .collect::<PyResult<Vec<_>>>()
            .map_err(|e| PyTypeError::new_err(format!("{name}: {e}")))
    } else {
        Ok(vec![])
    }
}

#[derive(Debug)]
pub struct CallbackStringPrint<'py>(&'py Bound<'py, PyAny>);

impl PrintWriter for CallbackStringPrint<'_> {
    fn stdout_write(&mut self, output: Cow<'_, str>) {
        // TODO PrintWriter needs to return a RunResult
        let s = output.into_pyobject(self.0.py()).unwrap();
        self.0.call1(("stdout", s)).unwrap();
    }

    fn stdout_push(&mut self, end: char) {
        let s = end.into_pyobject(self.0.py()).unwrap();
        self.0.call1(("stdout", s)).unwrap();
    }
}
