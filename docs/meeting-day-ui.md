# Meeting-day interface

The existing porcelain workspace now keeps the next meeting visible in the library. Prepare notes opens Notes through the atomic `prepare_calendar_meeting` command; Open notes reuses the same linked meeting. Both actions flush pending notes and recap edits first. A failed save or pending transcript correction keeps the user in the current document with their draft retained.

Calendar & reminders contains the device-calendar connection, explicit refresh, local ICS snapshot import and a default-off five-minute reminder preference. Imported snapshots show their date and explain reimporting when schedules change. Expired snapshots ask for an updated file. Stale device snapshots retain a refresh action. Solo timed events can be prepared manually; automatic reminders apply to timed events with guests or a meeting link. All-day and ended events stay out of the next-meeting flow.

A due reminder appears above the document with Prepare notes / Open notes and Dismiss. Preparing from the reminder dismisses that occurrence after opening its notes. Dismiss survives reload through the durable native contract. Recording, paused and processing meetings suppress the reminder. A local clock removes reminders outside their grace window even when the cached-day refresh fails. These actions never join a call or start recording.

The notification preference explains denied, unavailable and unknown system-delivery states while retaining in-app reminders. The browser preview explicitly says it cannot read calendars or send notifications. Enabling in the browser is not offered. Native fixtures reject unexpected methods, including recording, provider and calendar operations they do not explicitly model; their illustrative notification state never grants real system permission.

## Resource behavior

Only cached `get_meeting_day` is polled every 30 seconds. One in-flight refresh coalesces interval and focus requests. Capability queries run sequentially on initial load and focus, at most once per 30 seconds. Explicit connect, refresh and retry actions bypass that passive rate limit, while retaining single-flight ownership. Failures are handled independently, so a calendar capability error does not hide an imported snapshot. Reminder enable uses the actual status returned by its native command. Preparation and dismissal avoid unnecessary capability helpers. Every completed action requests a fresh cached read after any earlier in-flight poll has settled, so a pre-action response cannot win the final state. Explicit capability refresh follows the same ordering.

## Validation and evidence

The production build, seven unit tests and final full44-scenario browser regression suite passed. The earlier suite passed39 scenarios before the explicit-capability follow-ups; the final 13 meeting-day scenarios also verify that connecting updates the visible connection state immediately within the passive rate limit and that deferred pre-action polls cannot replace fresh prepare, dismiss or connection state. Meaningful regressions cover atomic reuse and draft preservation, failed-save retry, default-off enable, denied notifications, durable dismissal and disable, active-capture suppression, compact navigation, stale/expired snapshot recovery, slow single-flight polling, independent capability failure, local expiry and the browser preview boundary.

The one deterministic scan of `src/components/UpcomingMeetings.tsx` exited 0 with an empty findings array. Its raw result is `/private/tmp/tuck-meeting-day-detector.json`. No second detector run was used.

The initial desktop/390px capture batch was followed by one bounded confirmation to replace a malformed Chromium resize repaint and expose the scrolled preference states. Every final PNG was opened and visually validated:

- `.impeccable/review/meeting-day-desktop.png`
- `.impeccable/review/meeting-day-desktop-preferences.png`
- `.impeccable/review/meeting-day-compact-reminder.png`
- `.impeccable/review/meeting-day-compact-library.png`
- `.impeccable/review/meeting-day-compact-preferences.png`

These are illustrative native contract fixtures with enabled reminders, denied system notification permission, a stale device snapshot and an imported snapshot. The scrolled preference captures intentionally show a different part of the independently scrolling library. They establish interface composition and behavior, not hardware, permission, personal-calendar, account or notification-delivery qualification.

Independent Impeccable Assessment A returned **SHIP for interface design, 33/40**, with no material UI fixes requested. It retained three optional observations: compact actions remain below 44px touch guidance; the short late reminder grace could use more precise timing copy; and capability error recovery could identify each capability separately. The score covers this desktop product's reviewed interface, not native hardware behavior. Assessment B separately reviews the deterministic and raster evidence. Root records the combined disposition.
