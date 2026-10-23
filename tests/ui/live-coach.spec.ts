import { test, expect, type Page } from "@playwright/test";
import { defaults } from "../../src/lib/bridge";
import { exampleMeeting } from "../../src/lib/example";
import { installLeanLibraryPort } from "./support/lean-library";

async function fixture(page: Page, options: {hosted?: boolean; consent?: boolean; kind?: string; status?: string; late?: boolean} = {}) {
  await installLeanLibraryPort(page);
  await page.addInitScript(({ original, settings, options }) => {
    const w = window as any;
    const meetings = [{ ...original, isExample:false, kind:options.kind || "meeting", title:"Illustrative planning call", status:options.status || "recording", messages:[], tasks:[] },{ ...original, id:"second-call", isExample:false, title:"Another illustrative call",status:"complete",messages:[] }];
    let coach:any = {enabled:false,status:"off",selectedAgentIds:[],suggestions:[]};
    const calls:any[]=[];w.__coachCalls=calls;w.__coachSettings=settings;
    const callbacks=new Map<number,any>();let next=0;const listeners=new Map<string,number[]>();
    const emit=(event:string,payload:any)=>{for(const id of listeners.get("engine-event") || [])callbacks.get(id)?.({payload:{event,payload}});};
    w.__coachEmit=(patch:any)=>{coach={...coach,...patch};emit("coach_updated",{meetingId:original.id,coach:structuredClone(coach)});};
    w.__coachItems=()=>w.__coachEmit({suggestions:[{id:"tip-question",kind:"question",agentId:"quiet-agent",agentName:"Live coach",text:"Which decision needs an owner before we finish?",segmentId:original.segments[0].id,start:original.segments[0].start,quote:original.segments[0].text,createdAt:new Date().toISOString(),status:"open"},{id:"tip-follow-up",kind:"follow_up",agentId:"sales-agent",agentName:"Sales coach",text:"Send the revised proposal after the call.",segmentId:original.segments[1].id,start:original.segments[1].start,quote:original.segments[1].text,createdAt:new Date().toISOString(),status:"open"}]});
    w.__coachEnd=(status="complete")=>{meetings[0].status=status;coach={...coach,enabled:false,status:"stopped"};emit("meeting_updated",{meeting:structuredClone(meetings[0])});emit("coach_updated",{meetingId:original.id,coach:structuredClone(coach)});};
    w.__coachPause=()=>{meetings[0].status="paused";coach={...coach,status:"paused"};emit("meeting_updated",{meeting:structuredClone(meetings[0])});emit("coach_updated",{meetingId:original.id,coach:structuredClone(coach)});};
    w.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:"main"},currentWebview:{label:"main"}},transformCallback(cb:any){callbacks.set(++next,cb);return next;},async invoke(command:string,args:any){
      if(command==="plugin:event|listen"){listeners.set(args.event,[...(listeners.get(args.event)||[]),args.handler]);return args.handler;}
      if(command==="plugin:event|unlisten"){listeners.set(args.event,(listeners.get(args.event)||[]).filter(id=>id!==args.eventId));return;}
      if(command!=="engine_call")throw new Error(`Unexpected fixture command ${command}`);
      const {method,params:p={}}=args;calls.push({method,params:p});
      if(method==="bootstrap")return {meetings:structuredClone(meetings),settings,accounts:[],status:{engineVersion:"illustrative coach fixture",platform:"test",modelInstalled:true,modelDownloading:false,modelProgress:0,speakerModelInstalled:false,activeMeetingId:meetings[0].status==="recording"?original.id:null,recordingState:meetings[0].status,captureAvailable:true}};
      if(method==="get_meeting_day")return {events:[],snapshots:[],reminders:[],enabled:false,suppressed:false};
      if(method==="calendar_status")return {available:false,connected:false};
      if(method==="meeting_reminder_status")return {enabled:false,available:false,permission:"unavailable"};
      if(method==="get_live_coach"){
        if(p.meetingId!==original.id)return {enabled:false,status:"off",selectedAgentIds:[],suggestions:[]};
        if(options.late)return new Promise(resolve=>{w.__resolveCoachRead=()=>resolve({enabled:true,status:"waiting",selectedAgentIds:["quiet-agent"],suggestions:[{id:"stale",kind:"question",text:"Stale question from another meeting",agentName:"Live coach",start:0,status:"open"}]});});
        return structuredClone(coach);
      }
      if(method==="set_live_coach"){
        if(w.__enableFails)throw new Error("Your local AI model is not available. Start it and try again.");
        if(p.enabled && meetings[0].status!=="recording")throw new Error("Resume recording before turning on coaching.");
        coach={...coach,enabled:p.enabled,status:p.enabled?"waiting":"off",selectedAgentIds:p.agentIds || coach.selectedAgentIds};return structuredClone(coach);
      }
      if(method==="dismiss_coach_suggestion"){coach.suggestions=coach.suggestions.map((s:any)=>s.id===p.id?{...s,status:"dismissed"}:s);return structuredClone(coach);}
      if(method==="save_coach_follow_up"){
        if(w.__saveFollowUpFails)throw new Error("Your follow-up could not be saved. Try again.");
        const s=coach.suggestions.find((s:any)=>s.id===p.id);if(!s)throw new Error("Suggestion not found");
        if(s.status!=="saved"){(meetings[0].tasks as any[]).push({id:"saved-task",title:s.text,owner:"",dueDate:"",done:false,createdAt:new Date().toISOString()});s.status="saved";s.taskId="saved-task";}
        return {coach:structuredClone(coach),meeting:structuredClone(meetings[0])};
      }
      if(method==="update_settings"){if(w.__agentSaveFails)throw new Error("Settings could not be saved.");Object.assign(settings,p.changes);return structuredClone(settings);}
      if(method==="get_settings")return structuredClone(settings);
      if(method==="chat")return {};
      throw new Error(`Unexpected coach fixture method ${method}`);
    }};
  },{original:{...exampleMeeting(),id:"coach-call"},settings:{...defaults,provider:options.hosted?"chatgpt":"local",hostedConsent:!!options.consent,meetingAgents:[{id:"quiet-agent",name:"Live coach",instructions:"Notice useful questions and commitments.",pace:"quiet"},{id:"sales-agent",name:"Sales coach",instructions:"Watch for objections and useful next steps.",pace:"active"}]},options});
  await page.goto("/");
  await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Illustrative planning call");
}
const coach=(page:Page)=>page.getByRole("region",{name:"Live coach",exact:true});
const requests=(page:Page)=>page.evaluate(()=>(window as any).__coachCalls);
async function enable(page:Page){await coach(page).getByRole("button",{name:"Turn on for this call"}).click();await expect(coach(page).getByText("Listening for useful moments")).toBeVisible();}

test("coaching stays off until a per-call choice and never starts from opening the pane",async({page})=>{
  await fixture(page);await expect(coach(page).getByText("Off for this call")).toBeVisible();
  await expect(coach(page).getByRole("checkbox",{name:/Live coach/})).toBeChecked();await expect(coach(page).getByRole("checkbox",{name:/Sales coach/})).not.toBeChecked();
  expect((await requests(page)).filter((c:any)=>c.method==="set_live_coach")).toHaveLength(0);
  await coach(page).getByRole("checkbox",{name:/Sales coach/}).check();await enable(page);
  await expect(coach(page).getByText("Agents: Live coach, Sales coach",{exact:true})).toBeVisible();
  expect((await requests(page)).find((c:any)=>c.method==="set_live_coach").params).toEqual({meetingId:"coach-call",enabled:true,agentIds:["quiet-agent","sales-agent"]});
  await coach(page).getByRole("button",{name:"Turn off",exact:true}).click();await expect(coach(page).getByText("Off for this call")).toBeVisible();
});
test("hosted coaching names automatic transcript sharing and respects hosted permission",async({page})=>{
  await fixture(page,{hosted:true});await expect(coach(page).getByText("Automatically sends recent transcript excerpts, selected agent instructions and earlier tips to ChatGPT while enabled. This call only.")).toBeVisible();
  await expect(coach(page).getByRole("button",{name:"Turn on for this call"})).toHaveCount(0);await coach(page).getByRole("button",{name:"Allow hosted AI in Settings"}).click();await expect(page.getByRole("tab",{name:"Intelligence",exact:true})).toHaveAttribute("aria-selected","true");
  expect((await requests(page)).filter((c:any)=>c.method==="set_live_coach")).toHaveLength(0);
});
test("cited tips are dismissible and saved follow-ups become reviewed tasks without guessed dates",async({page})=>{
  await fixture(page);await enable(page);await page.evaluate(()=>(window as any).__coachItems());
  await expect(coach(page).getByText("Send the revised proposal after the call.",{exact:true})).toBeVisible();
  await page.evaluate(()=>(window as any).__saveFollowUpFails=true);await coach(page).getByRole("button",{name:"Save follow-up"}).click();await expect(coach(page).getByText(/Your follow-up could not be saved/)).toBeVisible();
  await page.evaluate(()=>(window as any).__saveFollowUpFails=false);await coach(page).getByRole("button",{name:"Save follow-up"}).click();await expect(coach(page).getByText(/Saved to this meeting’s Tasks/)).toBeVisible();await expect(coach(page).getByRole("button",{name:"Save follow-up"})).toHaveCount(0);
  await coach(page).getByRole("button",{name:/View transcript at/}).first().click();await expect(page.getByRole("tab",{name:/Transcript/})).toHaveAttribute("aria-selected","true");
  await coach(page).getByRole("button",{name:/Dismiss suggestion:/}).click();await expect(coach(page).getByText("Which decision needs an owner before we finish?",{exact:true})).toHaveCount(0);
  await page.getByRole("tab",{name:/Tasks/,exact:false}).click();await expect(page.getByText("Send the revised proposal after the call.",{exact:true})).toBeVisible();await expect(page.getByRole("textbox",{name:"Task owner"})).toHaveValue("");
});
test("pause and end stop visible coaching honestly while manual chat stays available",async({page})=>{
  await fixture(page);await enable(page);await page.evaluate(()=>(window as any).__coachItems());await page.evaluate(()=>(window as any).__coachPause());await expect(coach(page).getByText("Paused with recording")).toBeVisible();await expect(coach(page).getByRole("button",{name:"Turn off",exact:true})).toBeVisible();
  await page.evaluate(()=>(window as any).__coachEnd());await expect(coach(page).getByText("Call ended · coaching stopped")).toBeVisible();await expect(coach(page).getByRole("button",{name:"Turn on for this call"})).toHaveCount(0);await expect(page.getByRole("textbox",{name:"Ask Tuck about this conversation"})).toBeEnabled();
});
test("failed AI enable retains manual chat and an actionable inline retry",async({page})=>{
  await fixture(page);await page.evaluate(()=>(window as any).__enableFails=true);await coach(page).getByRole("button",{name:"Turn on for this call"}).click();await expect(coach(page).getByText(/Your local AI model is not available/)).toBeVisible();await expect(page.getByRole("textbox",{name:"Ask Tuck about this conversation"})).toBeEnabled();await page.evaluate(()=>(window as any).__enableFails=false);await coach(page).getByRole("button",{name:"Try again",exact:true}).click();await expect(coach(page).getByText("Listening for useful moments")).toBeVisible();
});
test("agent settings save custom instructions and failed saves preserve the draft",async({page})=>{
  await fixture(page);await coach(page).getByRole("button",{name:"Manage agents"}).click();const region=page.getByRole("region",{name:"Meeting agents",exact:true});await region.getByRole("button",{name:"Add agent"}).click();await region.getByRole("textbox",{name:"Agent name",exact:true}).fill("Pricing questions");await region.getByRole("textbox",{name:"Agent instructions"}).fill("Track unanswered pricing questions.");await region.getByRole("combobox",{name:"Agent pace"}).selectOption("active");await page.evaluate(()=>(window as any).__agentSaveFails=true);await region.getByRole("button",{name:"Save agent"}).click();await expect(region.getByRole("alert")).toContainText("Your edits are still here");await expect(region.getByRole("textbox",{name:"Agent name",exact:true})).toHaveValue("Pricing questions");await page.evaluate(()=>(window as any).__agentSaveFails=false);await region.getByRole("button",{name:"Save agent"}).click();await expect(region.getByText("Pricing questions",{exact:true})).toBeVisible();const agent=await page.evaluate(()=>(window as any).__coachSettings.meetingAgents.find((a:any)=>a.name==="Pricing questions"));expect(agent).toMatchObject({instructions:"Track unanswered pricing questions.",pace:"active"});expect((await requests(page)).filter((c:any)=>c.method==="set_live_coach")).toHaveLength(0);
});
test("a late read cannot place another meeting’s suggestions in the selected call",async({page})=>{
  await fixture(page,{late:true});await expect.poll(()=>page.evaluate(()=>!!(window as any).__resolveCoachRead)).toBe(true);await page.getByRole("button",{name:/Another illustrative call/}).click();await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Another illustrative call");await page.evaluate(()=>(window as any).__resolveCoachRead());await expect(page.getByText("Stale question from another meeting")).toHaveCount(0);
});
test("inactive meetings do not offer a coaching enable control",async({page})=>{
  await fixture(page,{status:"complete"});await expect(page.getByRole("button",{name:"Turn on for this call"})).toHaveCount(0);expect((await requests(page)).filter((c:any)=>c.method==="set_live_coach")).toHaveLength(0);
});

test("dictation never reads or enables meeting coaching",async({page})=>{
  await fixture(page,{kind:"dictation"});await expect(page.getByRole("region",{name:"Live coach",exact:true})).toHaveCount(0);await expect(page.getByRole("button",{name:"Turn on for this call"})).toHaveCount(0);expect((await requests(page)).filter((c:any)=>["get_live_coach","set_live_coach"].includes(c.method))).toHaveLength(0);
});

test("agent drafts survive view changes and cannot be replaced by another Edit",async({page})=>{
  await fixture(page);await coach(page).getByRole("button",{name:"Manage agents"}).click();
  const region=page.getByRole("region",{name:"Meeting agents",exact:true});
  await region.getByRole("button",{name:"Edit Sales coach"}).click();
  await region.getByRole("textbox",{name:"Agent instructions"}).fill("Keep this carefully written instruction while I navigate.");
  await expect(region.getByRole("button",{name:"Edit Live coach"})).toBeDisabled();
  await page.getByRole("tab",{name:"Privacy",exact:true}).click();
  await page.getByRole("tab",{name:"Intelligence",exact:true}).click();
  await expect(region.getByRole("textbox",{name:"Agent instructions"})).toHaveValue("Keep this carefully written instruction while I navigate.");
  await page.getByRole("button",{name:/Illustrative planning call/}).click();
  await coach(page).getByRole("button",{name:"Manage agents"}).click();
  await expect(region.getByRole("textbox",{name:"Agent instructions"})).toHaveValue("Keep this carefully written instruction while I navigate.");
  await region.getByRole("button",{name:"Cancel",exact:true}).click();
  await expect(region.getByRole("textbox",{name:"Agent instructions"})).toHaveCount(0);
  await expect(region.getByRole("button",{name:"Edit Live coach"})).toBeEnabled();
  await region.getByRole("button",{name:"Edit Sales coach"}).click();
  await expect(region.getByRole("textbox",{name:"Agent instructions"})).toHaveValue("Watch for objections and useful next steps.");
});
test("removing a saved agent requires an explicit inline choice",async({page})=>{
  await fixture(page);await coach(page).getByRole("button",{name:"Manage agents"}).click();
  const region=page.getByRole("region",{name:"Meeting agents",exact:true});
  await region.getByRole("button",{name:"Edit Sales coach"}).click();
  await region.getByRole("button",{name:"Remove agent",exact:true}).click();
  const confirm=region.getByRole("group",{name:"Confirm agent removal"});
  await expect(confirm).toContainText("Remove Sales coach?");
  expect((await requests(page)).filter((c:any)=>c.method==="update_settings")).toHaveLength(0);
  await confirm.getByRole("button",{name:"Keep agent"}).click();await expect(confirm).toHaveCount(0);
  await region.getByRole("button",{name:"Remove agent",exact:true}).click();
  await confirm.getByRole("button",{name:"Confirm removal"}).click();
  await expect(region.getByRole("button",{name:"Edit Sales coach"})).toHaveCount(0);
  await expect(region.getByRole("button",{name:"Edit Live coach"})).toBeEnabled();
});
test("new advice has a short polite announcement and background failures explain automatic recovery",async({page})=>{
  await fixture(page);await enable(page);
  await expect(coach(page).getByText("Agents: Live coach",{exact:true})).toBeVisible();
  await page.evaluate(()=>(window as any).__coachItems());
  const announcement=coach(page).getByRole("status").filter({hasText:"2 new live coach suggestions."});
  await expect(announcement).toHaveAttribute("aria-live","polite");
  await expect(announcement).toHaveAttribute("aria-atomic","true");
  await page.evaluate(()=>(window as any).__coachEmit({status:"unavailable",message:"Your local AI model stopped. Start it and try again."}));
  await expect(coach(page).getByText(/Live coach checks again automatically/)).toBeVisible();
  await expect(coach(page).getByText(/Manual chat uses the same AI connection/)).toBeVisible();
  await expect(coach(page).getByRole("button",{name:"Try again",exact:true})).toHaveCount(0);
  await expect(coach(page).getByRole("button",{name:"Check AI settings"})).toBeVisible();
  expect((await requests(page)).filter((c:any)=>c.method==="set_live_coach")).toHaveLength(1);
  await expect(page.getByRole("textbox",{name:"Ask Tuck about this conversation"})).toBeEnabled();
});
test("compact live help controls have usable touch targets",async({page})=>{
  await fixture(page);await page.setViewportSize({width:390,height:900});
  await page.getByRole("button",{name:"Ask Tuck",exact:true}).click();
  for(const button of [coach(page).getByRole("button",{name:"Turn on for this call"}),coach(page).getByRole("button",{name:"Manage agents"})])expect((await button.boundingBox())!.height).toBeGreaterThanOrEqual(40);
  await enable(page);await page.evaluate(()=>(window as any).__coachItems());
  for(const button of [coach(page).getByRole("button",{name:"Turn off",exact:true}),coach(page).getByRole("button",{name:/View transcript at/}).first(),coach(page).getByRole("button",{name:/Dismiss suggestion:/}).first(),coach(page).getByRole("button",{name:"Save follow-up"})])expect((await button.boundingBox())!.height).toBeGreaterThanOrEqual(40);
});

test("bounded coach and agent settings visual evidence",async({page})=>{
  test.skip(!process.env.TUCK_CAPTURE_COACH && !process.env.TUCK_CAPTURE_COACH_CONFIRM,"Run once for the bounded visual batch.");
  const prefix=process.env.TUCK_CAPTURE_COACH_CONFIRM?"live-coach-confirm":"live-coach";
  await fixture(page);await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;pointer-events:none}'});
  for(const width of [1320,390]){
    await page.setViewportSize({width,height:900});if(width===390)await page.getByRole("button",{name:"Ask Tuck",exact:true}).click();
    await page.screenshot({path:`.impeccable/review/${prefix}-off-${width}.png`});await enable(page);await page.screenshot({path:`.impeccable/review/${prefix}-listening-${width}.png`});await page.evaluate(()=>(window as any).__coachItems());await expect(coach(page).getByText("Send the revised proposal after the call.",{exact:true})).toBeVisible();await page.screenshot({path:`.impeccable/review/${prefix}-suggestions-${width}.png`});await page.evaluate(()=>(window as any).__coachEmit({status:"unavailable",message:"Your local AI model stopped. Start it and try again."}));await page.screenshot({path:`.impeccable/review/${prefix}-error-${width}.png`});await page.evaluate(()=>(window as any).__coachEnd());await expect(coach(page).getByText("Call ended · coaching stopped")).toBeVisible();await page.screenshot({path:`.impeccable/review/${prefix}-ended-${width}.png`});
    await page.getByRole("button",{name:/^Settings/}).isVisible().then(async visible=>{if(!visible)await page.getByRole("button",{name:"Library",exact:true}).click();});await page.getByRole("button",{name:/^Settings/}).click();const region=page.getByRole("region",{name:"Meeting agents",exact:true});if(await region.getByRole("textbox",{name:"Agent instructions"}).count())await region.getByRole("button",{name:"Cancel",exact:true}).click();await region.getByRole("button",{name:"Edit Sales coach"}).click();await region.scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/${prefix}-agents-${width}.png`});
    if(process.env.TUCK_CAPTURE_COACH_CONFIRM){await region.getByRole("button",{name:"Remove agent",exact:true}).click();await region.getByRole("group",{name:"Confirm agent removal"}).scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/${prefix}-agents-removal-${width}.png`});await region.getByRole("button",{name:"Keep agent"}).click();}
    if(width===1320){await page.getByRole("button",{name:/Illustrative planning call/}).click();await page.evaluate(() => {const w=window as any;w.__coachEnd("recording");w.__coachEmit({enabled:false,status:"off",suggestions:[]});});}
  }
});
