# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

"""OpenFeature adapter. The application owns the client's async init and cleanup."""

from collections.abc import Mapping, Sequence
from typing import Never

from openfeature.evaluation_context import EvaluationContext
from openfeature.exception import (
    FlagNotFoundError,
    GeneralError,
    InvalidContextError,
    ProviderFatalError,
    ProviderNotReadyError,
    TypeMismatchError,
)
from openfeature.flag_evaluation import FlagResolutionDetails, FlagValueType, Reason
from openfeature.provider import AbstractProvider, Metadata

from . import (
    Client,
    Decision,
    KeyRevokedError,
    PartlyError,
    PolicyNotReadyError,
)
from . import (
    FlagNotFoundError as PartlyFlagNotFound,
)
from . import (
    InvalidContextError as PartlyInvalidContext,
)


class PartlyProvider(AbstractProvider):
    def __init__(self, client: Client) -> None:
        super().__init__()
        self._client = client
        self._ready = False

    def get_metadata(self) -> Metadata:
        return Metadata(name="Partly")

    def initialize(self, evaluation_context: EvaluationContext) -> None:
        # Python's OpenFeature lifecycle is synchronous; never run network work here.
        self._ready = False
        try:
            self._client.has_flag("")
        except PartlyError as error:
            self._raise(error)
        self._ready = True

    def shutdown(self) -> None:
        self._ready = False

    @staticmethod
    def _raise(error: Exception) -> Never:
        if isinstance(error, KeyRevokedError):
            raise ProviderFatalError("API Key revoked") from error
        if isinstance(error, PolicyNotReadyError):
            raise ProviderNotReadyError("Await Partly client.init() before registering the provider") from error
        if isinstance(error, PartlyFlagNotFound):
            raise FlagNotFoundError("Flag is not defined") from error
        if isinstance(error, (PartlyInvalidContext, TypeError, ValueError, OverflowError)):
            raise InvalidContextError("Context does not satisfy the flag's requirements") from error
        raise GeneralError("Partly evaluation failed") from error

    def _check(self, flag_key: str) -> None:
        if not self._ready:
            raise ProviderNotReadyError("Partly provider is not initialized")
        try:
            exists = self._client.has_flag(flag_key)
        except PartlyError as error:
            self._raise(error)
        if not exists:
            raise FlagNotFoundError("Flag is not defined")

    def resolve_boolean_details(self, flag_key: str, default_value: bool,
                                evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[bool]:
        self._check(flag_key)
        context: dict[str, str | int | float | bool] = {}
        if evaluation_context is not None:
            # Preserve flat scalar attributes; structured fields are not rule inputs in Partly.
            context = {k: v for k, v in evaluation_context.attributes.items() if isinstance(v, (str, int, float, bool))}
            if evaluation_context.targeting_key is not None:
                context["id"] = evaluation_context.targeting_key
        try:
            value = self._client.evaluate(flag_key, context=context) is Decision.ENABLED
        except (PartlyError, TypeError, ValueError, OverflowError) as error:
            self._raise(error)
        return FlagResolutionDetails(value=value, variant="enabled" if value else "disabled", reason=Reason.CACHED)

    def _unsupported(self, flag_key: str) -> Never:
        self._check(flag_key)
        raise TypeMismatchError("Partly flags are Boolean")

    def resolve_string_details(self, flag_key: str, default_value: str,
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[str]:
        self._unsupported(flag_key)

    def resolve_integer_details(self, flag_key: str, default_value: int,
                                evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[int]:
        self._unsupported(flag_key)

    def resolve_float_details(self, flag_key: str, default_value: float,
                              evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[float]:
        self._unsupported(flag_key)

    def resolve_object_details(self, flag_key: str, default_value: Sequence[FlagValueType] | Mapping[str, FlagValueType],
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[Sequence[FlagValueType] | Mapping[str, FlagValueType]]:
        self._unsupported(flag_key)
