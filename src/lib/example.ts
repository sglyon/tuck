import type { Meeting } from "./types";
export function exampleMeeting(): Meeting {
  const id = crypto.randomUUID();
  const rows: [number, number, string, string][] = [
    [
      0,
      15,
      "you",
      "Thanks for making time, Maya. I'd love to understand how your team handles customer handoffs today, and where things get stuck.",
    ],
    [
      16,
      38,
      "speaker-1",
      "Right now it's a mix of Salesforce, Notion, and a lot of Slack messages. The context gets lost between the sales call and onboarding. We have twelve account executives, and everyone does it a little differently.",
    ],
    [
      39,
      54,
      "you",
      "So the information exists, but putting it together is the hard part. What does that cost the team on a typical deal?",
    ],
    [
      55,
      78,
      "speaker-1",
      "Our onboarding leads spend about forty-five minutes reconstructing each deal. The bigger problem is the promises. Someone agrees to a custom integration on a call, and the delivery team hears about it two weeks later.",
    ],
    [
      79,
      104,
      "speaker-2",
      "From the technical side, we'd need the notes to land in our existing Notion database. We can't ask everyone to adopt another tool. And for anything involving customer data, local processing is a real advantage.",
    ],
    [
      105,
      124,
      "you",
      "That makes sense. We can start with a simple workflow: capture the conversation, pull out commitments and open questions, and send those to the account's Notion page.",
    ],
    [
      125,
      146,
      "speaker-1",
      "That's the workflow I want to pilot. We have a new onboarding cohort starting November ninth. Could we try this with three account executives before then?",
    ],
    [
      147,
      170,
      "speaker-2",
      "I'll share the database structure and our security questionnaire by Friday. We should also test speaker separation when two people are in the same room.",
    ],
    [
      171,
      194,
      "you",
      "I'll send a pilot outline tomorrow, with the three-person scope and success criteria. Let's measure time spent on handoffs and whether the delivery team gets every customer commitment.",
    ],
    [
      195,
      215,
      "speaker-1",
      "Perfect. If it works, we can bring in the rest of the sales team. Let's do a thirty-minute check-in next Thursday to review the first few calls.",
    ],
  ];
  return {
    id,
    title: "Northstar / discovery call",
    createdAt: new Date().toISOString(),
    duration: 215,
    status: "complete",
    template: "sales",
    starred: false,
    isExample: true,
    notionUrl: null,
    notionStatus: "idle",
    notes:
      "Pilot: 3 account executives\nFocus on handoffs, not another source of truth.\n\nAsk Maya which commitments are most often missed.\nSecurity review before recording real customer calls.",
    summary:
      "## A clearer handoff, without another tool\n\nNorthstar’s sales-to-onboarding handoff loses context between Salesforce, Notion, and Slack. Maya wants a small pilot that makes customer commitments visible to the delivery team. [00:16]\n\n### What matters\n\n- Onboarding leads spend roughly 45 minutes reconstructing each deal; missed promises create the bigger risk. [00:55]\n- Notes must arrive in the existing Notion database. Local processing matters for customer data. [01:19]\n- Start with three account executives before the November 9 onboarding cohort. [02:05]\n\n### Next steps\n\n- **You** — Send the pilot outline tomorrow, including scope and success criteria. [02:51]\n- **Eli** — Share the database structure and security questionnaire by Friday. [02:27]\n- **Maya + you** — Review the first calls in a 30-minute check-in next Thursday. [03:15]\n\n### Still open\n\nCan speaker separation handle two people in the same room? Test during the pilot. [02:27]",
    segments: rows.map(([start, end, speakerId, text]) => ({
      id: crypto.randomUUID(),
      meetingId: id,
      start,
      end,
      speakerId,
      text,
      source: speakerId === "you" ? "microphone" : "system",
    })),
    speakers: [
      { id: "you", name: "You", color: 0 },
      { id: "speaker-1", name: "Maya Chen", color: 1 },
      { id: "speaker-2", name: "Eli Brooks", color: 2 },
    ],
    messages: [],
    bookmarks: [{ id: crypto.randomUUID(), time: 125, label: "Pilot scope" }],
  };
}
