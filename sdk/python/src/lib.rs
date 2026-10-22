// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use partly_core::{Context, Decision, EvaluationError, Policy, MAX_POLICY_BYTES};
use pyo3::{
    create_exception,
    exceptions::PyException,
    prelude::*,
    types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple},
};
use serde_json::{Map, Number, Value};

create_exception!(_native, PartlyError, PyException);
create_exception!(_native, FlagNotFoundError, PartlyError);
create_exception!(_native, InvalidContextError, PartlyError);
create_exception!(_native, ContextTypeError, InvalidContextError);
create_exception!(_native, InvalidPolicyError, PartlyError);

#[pyclass(frozen)]
struct Snapshot {
    policy: Policy,
}

fn evaluation_error(py: Python<'_>, error: EvaluationError) -> PyErr {
    match &error {
        EvaluationError::FlagNotFound(_) => FlagNotFoundError::new_err(error.to_string()),
        EvaluationError::InvalidContext { .. } => InvalidContextError::new_err(error.to_string()),
        EvaluationError::ContextType {
            flag_key,
            k,
            op,
            expected_type,
            actual_type,
        } => {
            let exception = ContextTypeError::new_err(error.to_string());
            let value = exception.value(py);
            for (name, detail) in [
                ("flag_key", flag_key.as_str()),
                ("k", k.as_str()),
                ("expected_type", *expected_type),
                ("actual_type", *actual_type),
            ] {
                if let Err(error) = value.setattr(name, detail) {
                    return error;
                }
            }
            if let Err(error) = value.setattr("op", *op) {
                return error;
            }
            exception
        }
    }
}

fn invalid_context(key: &str, field: &str, reason: &'static str) -> PyErr {
    InvalidContextError::new_err(
        EvaluationError::InvalidContext {
            flag_key: key.into(),
            k: field.into(),
            reason,
        }
        .to_string(),
    )
}

fn context_values(key: &str, context: &Bound<'_, PyAny>) -> PyResult<Context> {
    let dictionary = context
        .cast::<PyDict>()
        .map_err(|_| InvalidContextError::new_err("context must be a flat dictionary"))?;
    dictionary
        .iter()
        .map(|(k, v)| {
            let k = k
                .extract::<String>()
                .map_err(|_| InvalidContextError::new_err("context keys must be strings"))?;
            let value = if v.is_instance_of::<PyBool>() {
                Value::Bool(v.extract()?)
            } else if v.is_instance_of::<PyInt>() {
                if let Ok(number) = v.extract::<i64>() {
                    Value::Number(number.into())
                } else if let Ok(number) = v.extract::<u64>() {
                    Value::Number(number.into())
                } else {
                    return Err(invalid_context(
                        key,
                        &k,
                        "integer is outside the supported 64-bit range",
                    ));
                }
            } else if v.is_instance_of::<PyFloat>() {
                let number = Number::from_f64(v.extract()?)
                    .ok_or_else(|| invalid_context(key, &k, "numbers must be finite"))?;
                Value::Number(number)
            } else if v.is_instance_of::<PyString>() {
                Value::String(
                    v.extract()
                        .map_err(|_| invalid_context(key, &k, "text must contain valid Unicode"))?,
                )
            } else if v.is_none() {
                Value::Null
            } else if v.is_instance_of::<PyList>() || v.is_instance_of::<PyTuple>() {
                Value::Array(vec![])
            } else {
                // Keep only the type so the evaluator reports the rule's expected type safely.
                Value::Object(Map::new())
            };
            Ok((k, value))
        })
        .collect()
}

#[pymethods]
impl Snapshot {
    fn contains_flag(&self, key: &str) -> bool {
        self.policy.flags.contains_key(key)
    }

    #[new]
    fn new(py: Python<'_>, body: &[u8]) -> PyResult<Self> {
        Ok(Self {
            policy: py
                .detach(|| Policy::from_json(body))
                .map_err(InvalidPolicyError::new_err)?,
        })
    }

    #[getter]
    fn project(&self) -> &str {
        &self.policy.project
    }

    #[getter]
    fn application_id(&self) -> &str {
        &self.policy.application_id
    }

    #[getter]
    fn environment(&self) -> &str {
        &self.policy.environment
    }

    fn evaluate(&self, py: Python<'_>, key: &str, context: &Bound<'_, PyAny>) -> PyResult<bool> {
        if !self.policy.flags.contains_key(key) {
            return Err(evaluation_error(
                py,
                EvaluationError::FlagNotFound(key.into()),
            ));
        }
        let context = context_values(key, context)?;
        self.policy
            .evaluate(key, &context)
            .map(|decision| decision == Decision::Enabled)
            .map_err(|error| evaluation_error(py, error))
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Snapshot>()?;
    m.add("MAX_POLICY_BYTES", MAX_POLICY_BYTES)?;
    m.add("PartlyError", m.py().get_type::<PartlyError>())?;
    m.add("FlagNotFoundError", m.py().get_type::<FlagNotFoundError>())?;
    m.add(
        "InvalidContextError",
        m.py().get_type::<InvalidContextError>(),
    )?;
    m.add("ContextTypeError", m.py().get_type::<ContextTypeError>())?;
    m.add(
        "InvalidPolicyError",
        m.py().get_type::<InvalidPolicyError>(),
    )?;
    Ok(())
}
