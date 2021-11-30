from __future__ import annotations

from typing import (
    AbstractSet,
    Any,
    Callable,
    Generic,
    Iterable,
    Optional,
    Protocol,
    Sequence,
    Tuple,
    TypeVar,
)

from .graphlib2 import CycleError
from .graphlib2 import TopologicalSorter as _TopologicalSorter

_KT_co = TypeVar("_KT_co", covariant=True)
_VT_co = TypeVar("_VT_co", covariant=True)
_T = TypeVar("_T")


class SupportsItems(Protocol[_KT_co, _VT_co]):
    def items(self) -> AbstractSet[Tuple[_KT_co, _VT_co]]:
        ...


class _DefaultNodeIdFactory:
    __slots__ = "current_count"
    current_count: int

    def __init__(self) -> None:
        self.current_count = 0

    def __call__(self, dep: Any) -> int:
        res = self.current_count
        self.current_count += 1
        return res


class TopologicalSorter(Generic[_T]):
    __slots__ = ("_ts", "_node_id_factory")

    def __init__(
        self,
        graph: Optional[SupportsItems[_T, Iterable[_T]]] = None,
        node_id_factory: Optional[Callable[[_T], int]] = None,
    ) -> None:
        node_id_factory = node_id_factory or _DefaultNodeIdFactory()
        self._ts: _TopologicalSorter[_T] = _TopologicalSorter(graph, node_id_factory)

    def add(self, node: _T, *predecessors: _T) -> None:
        self._ts.add(node, predecessors)

    def get_ready(self) -> Tuple[_T, ...]:
        return self._ts.get_ready()

    def done(self, *nodes: _T) -> None:
        self._ts.done(nodes)

    def is_active(self) -> bool:
        return self._ts.is_active()

    def prepare(self) -> None:
        self._ts.prepare()

    def static_order(self) -> Iterable[_T]:
        return self._ts.static_order()

    def copy(self: TopologicalSorter[_T]) -> TopologicalSorter[_T]:
        new: TopologicalSorter[_T] = object.__new__(TopologicalSorter)
        new._ts = self._ts.copy()
        return new

    def get_ids(self, *nodes: _T) -> Sequence[int]:
        return self._ts.get_ids(nodes)

    def done_by_id(self, *nodes: int) -> None:
        self._ts.done_by_id(nodes)

    def remove_nodes(self, nodes: Iterable[_T]) -> None:
        self._ts.remove_nodes(nodes)

    def remove_nodes_by_id(self, nodes: Iterable[int]) -> None:
        self._ts.remove_nodes_by_id(nodes)


__all__ = ("TopologicalSorter", "CycleError")
