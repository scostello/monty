from typing import Any, Callable, Literal, final

from typing_extensions import Self

__all__ = ['Monty', 'MontyComplete', 'MontyProgress', 'ResourceLimits']

@final
class Monty:
    """
    A sandboxed Python interpreter instance.

    Parses and compiles Python code on initialization, then can be run
    multiple times with different input values. This separates the parsing
    cost from execution, making repeated runs more efficient.
    """

    def __new__(
        cls,
        code: str,
        *,
        script_name: str = 'main.py',
        inputs: list[str] | None = None,
        external_functions: list[str] | None = None,
    ) -> Self:
        """
        Create a new Monty interpreter by parsing the given code.

        Arguments:
            code: Python code to execute
            script_name: Name used in tracebacks and error messages
            inputs: List of input variable names available in the code
            external_functions: List of external function names the code can call

        Raises:
            SyntaxError: If the code cannot be parsed
        """

    def run(
        self,
        *,
        inputs: dict[str, Any] | None = None,
        limits: ResourceLimits | None = None,
        external_functions: dict[str, Callable[..., Any]] | None = None,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
    ) -> Any:
        """
        Execute the code and return the result.

        Arguments:
            inputs: Dict of input variable values (must match names from __init__)
            limits: Optional resource limits configuration
            external_functions: Dict of external function callbacks (must match names from __init__)
            print_callback: Optional callback for print output

        Returns:
            The result of the last expression in the code

        Raises:
            Various Python exceptions matching what the code would raise
        """

    def start(
        self,
        *,
        inputs: dict[str, Any] | None = None,
        limits: ResourceLimits | None = None,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
    ) -> MontyProgress | MontyComplete:
        """
        Start the code execution and return a progress object, or completion.

        This allows you to iteratively run code and parse/resume whenever an external function is called.

        Arguments:
            inputs: Dict of input variable values (must match names from __init__)
            limits: Optional resource limits configuration
            print_callback: Optional callback for print output

        Returns:
            MontyProgress if an external function call is pending,
            MontyComplete if execution finished without external calls.
        """

    def __repr__(self) -> str: ...

@final
class MontyProgress:
    """
    Represents a paused execution waiting for an external function call return value.

    Contains information about the pending external function call and allows
    resuming execution with the return value.
    """

    @property
    def script_name(self) -> str:
        """The name of the script being executed."""

    @property
    def function_name(self) -> str:
        """The name of the external function being called."""

    @property
    def args(self) -> tuple[Any, ...]:
        """The positional arguments passed to the external function."""

    @property
    def kwargs(self) -> dict[str, Any]:
        """The keyword arguments passed to the external function."""

    def resume(self, return_value: Any) -> MontyProgress | MontyComplete:
        """
        Resume execution with the return value from an external function call.

        Resume may only be called once on each MontyProgress instance.

        Arguments:
            return_value: The value to return from the external function call.

        Returns:
            MontyProgress if another external function call is pending,
            MontyComplete if execution finished.

        Raises:
            RuntimeError: If execution has already completed.
        """

    def __repr__(self) -> str: ...

@final
class MontyComplete:
    """The result of a completed code execution."""

    @property
    def output(self) -> Any:
        """The final output value from the executed code."""

    def __repr__(self) -> str: ...

@final
class ResourceLimits:
    """
    Configuration for resource limits during code execution.

    All limits are optional. Set to None to disable a specific limit.
    """

    max_allocations: int | None
    """Maximum number of heap allocations allowed."""

    max_duration_secs: float | None
    """Maximum execution time in seconds."""

    max_memory: int | None
    """Maximum heap memory in bytes."""

    gc_interval: int | None
    """Run garbage collection every N allocations."""

    max_recursion_depth: int | None
    """Maximum function call stack depth (default: 1000)."""

    def __new__(
        cls,
        *,
        max_allocations: int | None = None,
        max_duration_secs: float | None = None,
        max_memory: int | None = None,
        gc_interval: int | None = None,
        max_recursion_depth: int | None = ...,
    ) -> Self:
        """
        Create a new ResourceLimits configuration.

        Arguments:
            max_allocations: Maximum number of heap allocations
            max_duration_secs: Maximum execution time in seconds
            max_memory: Maximum heap memory in bytes
            gc_interval: Run garbage collection every N allocations
            max_recursion_depth: Maximum function call depth (default: 1000)
        """

    def __repr__(self) -> str: ...
