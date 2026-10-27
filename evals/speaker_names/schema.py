from typing import Literal
from pydantic import BaseModel, ConfigDict, Field, StrictStr, model_validator


class Suggestion(BaseModel):
    model_config = ConfigDict(extra="forbid")
    speakerId: StrictStr = Field(min_length=1)
    name: StrictStr = Field(min_length=1, max_length=80)
    strength: Literal["explicit", "supported", "tentative"]
    evidenceSegmentIds: list[StrictStr] = Field(min_length=1)


class Suggestions(BaseModel):
    model_config = ConfigDict(extra="forbid")
    suggestions: list[Suggestion]

    @model_validator(mode="after")
    def unique_speakers(self):
        ids = [s.speakerId for s in self.suggestions]
        if len(set(ids)) != len(ids):
            raise ValueError("One suggestion per acoustic voice; conflicting/duplicate names must abstain.")
        return self
