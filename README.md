# Monty

A sandboxed, snapshotable Python interpreter written in Rust.

Monty is a **sandboxed Python interpreter** written in Rust. Unlike embedding CPython or using PyO3,
Monty implements its own runtime from scratch.

The goal is to provide:
* complete safety - no access to the host environment, filesystem or network
* safe access to specific methods on the host
* snapshotting and iterative execution for long running host functions

## Usage

### Python

```python
import monty

code = """
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)
"""

m = monty.Monty(code, inputs=['x'])
print(m.run(inputs={'x': 10}))
#> 55
```

#### Iterative Execution with External Functions

Use `start()` and `resume()` to handle external function calls iteratively,
giving you control over each call:

```python
import monty

code = """
data = fetch(url)
len(data)
"""

m = monty.Monty(code, inputs=['url'], external_functions=['fetch'])

# Start execution - pauses when fetch() is called
result = m.start(inputs={'url': 'https://example.com'})

print(type(result))
#> <class 'builtins.MontyProgress'>
print(result.function_name)  # fetch
#> fetch
print(result.args)
#> ('https://example.com',)

# Perform the actual fetch, then resume with the result
result = result.resume('hello world')

print(type(result))
#> <class 'builtins.MontyComplete'>
print(result.output)
#> 11
```

### Rust

```rust
use monty::{RunSnapshot, MontyObject, NoLimitTracker, StdPrint};

let code = r#"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)
"#;

let snapshot = RunSnapshot::new(code.to_owned(), "fib.py", vec!["x".to_owned()], vec![]).unwrap();
let result = snapshot.run_no_snapshot(vec![MontyObject::Int(10)], NoLimitTracker::default(), &mut StdPrint).unwrap();
assert_eq!(result, MontyObject::Int(55));
```
