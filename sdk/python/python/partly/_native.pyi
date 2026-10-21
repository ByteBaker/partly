# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

from typing import Self, final

__all__ = ["Snapshot", "MAX_POLICY_BYTES", "PartlyError", "FlagNotFoundError", "InvalidContextError", "ContextTypeError", "InvalidPolicyError"]

MAX_POLICY_BYTES: int

class PartlyError(Exception): ...
class FlagNotFoundError(PartlyError): ...
class InvalidContextError(PartlyError): ...
class ContextTypeError(InvalidContextError):
    flag_key: str
    k: str
    op: str | None
    expected_type: str
    actual_type: str
class InvalidPolicyError(PartlyError): ...

@final
class Snapshot:
    def __new__(cls, body: bytes) -> Self: ...
    @property
    def project(self) -> str: ...
    @property
    def environment(self) -> str: ...
    @property
    def application_id(self) -> str: ...
    def evaluate(self, key: str, context: object) -> bool: ...
