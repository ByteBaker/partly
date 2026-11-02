# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

"""OpenFeature adapter. The application owns the client's async init and cleanup."""

from collections.abc import Mapping, Sequence
from typing import Never, cast

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
    ResultTypeError,
)
from . import (
    FlagNotFoundError as PartlyFlagNotFound,
)
from . import (
    InvalidContextError as PartlyInvalidContext,
)
from .values import Resolution


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
        if isinstance(error, ResultTypeError):
            raise TypeMismatchError("Requested result type does not match the flag") from error
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

    def _resolve(self, flag_key: str, kind: str, evaluation_context: EvaluationContext | None) -> FlagResolutionDetails[FlagValueType]:
        self._check(flag_key)
        context: dict[str, str | int | float | bool] = {}
        if evaluation_context is not None:
            # Preserve flat scalar attributes; structured fields are not rule inputs in Partly.
            context = {k: v for k, v in evaluation_context.attributes.items() if isinstance(v, (str, int, float, bool))}
            if evaluation_context.targeting_key is not None:
                context["id"] = evaluation_context.targeting_key
        try:
            if kind == "boolean":
                value = self._client.evaluate(flag_key, context=context) is Decision.ENABLED
                return FlagResolutionDetails(value=value, variant="enabled" if value else "disabled",
                                             reason=Reason.CACHED, flag_metadata={})
            result: Resolution = self._client.resolve(flag_key, kind, context=context)
        except (PartlyError, TypeError, ValueError, OverflowError) as error:
            self._raise(error)
        return FlagResolutionDetails(value=result["value"], variant=result["variant"], reason=Reason.CACHED,
                                     flag_metadata={"partly.result_type": result["named_type"]} if "named_type" in result else {})

    def resolve_boolean_details(self, flag_key: str, default_value: bool,
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[bool]:
        return cast("FlagResolutionDetails[bool]", self._resolve(flag_key, "boolean", evaluation_context))

    def resolve_string_details(self, flag_key: str, default_value: str,
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[str]:
        return cast("FlagResolutionDetails[str]", self._resolve(flag_key, "string", evaluation_context))

    def resolve_integer_details(self, flag_key: str, default_value: int,
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[int]:
        return cast("FlagResolutionDetails[int]", self._resolve(flag_key, "integer", evaluation_context))

    def resolve_float_details(self, flag_key: str, default_value: float,
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[float]:
        return cast("FlagResolutionDetails[float]", self._resolve(flag_key, "float", evaluation_context))

    def resolve_object_details(self, flag_key: str, default_value: Sequence[FlagValueType] | Mapping[str, FlagValueType],
                               evaluation_context: EvaluationContext | None = None) -> FlagResolutionDetails[Sequence[FlagValueType] | Mapping[str, FlagValueType]]:
        return cast("FlagResolutionDetails[Sequence[FlagValueType] | Mapping[str, FlagValueType]]", self._resolve(flag_key, "object", evaluation_context))
