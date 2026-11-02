# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

from typing import NotRequired, TypedDict

type Context = dict[str, str | int | float | bool]
type Record = dict[str, str | int | float | bool]


class Resolution(TypedDict):
    value: str | int | float | bool | Record
    kind: str
    variant: str
    named_type: NotRequired[str]
