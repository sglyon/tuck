import { test, expect, type Page } from "@playwright/test";
import { defaults } from "../../src/lib/bridge";
import { exampleMeeting } from "../../src/lib/example";
import { installLeanLibraryPort } from "./support/lean-library";

async function port(page: Page, options: {ready?: boolean; active?: boolean; configured?: boolean; manual?: boolean} = {}) {
  await installLeanLibraryPort(page);
  await page.addInitScript(({original, settings, options}) => {
    const w = window as any;
    const meetings: any[] = [{...original, isExample:false, title:"Planning notes", notes:"Existing notes", status: options.active ? "recording" : "complete"}];
    const calls: any[] = []; w.__flowCalls = calls; w.__flowMeetings = meetings;
    let account = {configured: options.configured !== false, connected: !!options.manual, connecting:false, mode:options.manual ? "manual" : null, workspaceName:null, workspaceId:null};
    let resolveConnect: any;
    const callbacks = new Map<number, any>(); let next = 0; const listeners = new Map<string, number[]>();
    const emit = (event: string, payload: any) => { for (const id of listeners.get("engine-event") || []) callbacks.get(id)?.({payload:{event,payload}}); };
    const status = {engineVersion:"strict illustrative fixture",platform:"test",modelInstalled:true,modelDownloading:false,modelProgress:0,speakerModelInstalled:false,activeMeetingId:options.active ? original.id : null,recordingState:options.active ? "recording":"idle",captureAvailable:true};
    w.__finishNotion = (denied = false) => {account = {...account, connected:!denied,connecting:false,mode:denied ? null:"oauth",workspaceName:denied ? null:"Illustrative workspace" as any,workspaceId:denied ? null:"workspace-test" as any}; settings.hasNotionToken = !denied; if (denied) resolveConnect.reject(new Error("Authorization was denied. Connect again when ready.")); else resolveConnect.resolve({...account});};
    w.__dictationSegment = () => { const m=meetings.find(m=>m.kind==="dictation"); const segment={id:"spoken",meetingId:m.id,start:0,end:2,text:"Send the updated proposal tomorrow.",speakerId:"",source:"microphone"};m.segments=[segment];emit("segment",{meetingId:m.id,segment});emit("meeting_updated",{meeting:m}); };
    Object.defineProperty(navigator, "clipboard", {configurable:true,value:{writeText:async(text:string)=>{if(w.__copyFails)throw new Error("Clipboard denied");w.__copied=text;}}});
    w.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:"main"},currentWebview:{label:"main"}},transformCallback(cb:any){callbacks.set(++next,cb);return next;},async invoke(command:string,args:any){
      if(command==="plugin:event|listen"){listeners.set(args.event,[...(listeners.get(args.event)||[]),args.handler]);return args.handler;}
      if(command==="plugin:event|unlisten")return;
      if(command!=="engine_call")throw new Error(`Unexpected native fixture command: ${command}`);
      const {method,params:p={}}=args;calls.push({method,params:p});
      if(method==="bootstrap")return {meetings:structuredClone(meetings),settings,accounts:[],status};
      if(method==="get_status")return status;
      if(method==="update_settings"){Object.assign(settings,p.changes);return {...settings};}
      if(method==="get_meeting")return structuredClone(meetings.find(m=>m.id===p.meetingId));
      if(method==="get_meeting_day")return {events:[],snapshots:[],reminders:[],enabled:false,suppressed:false};
      if(method==="calendar_status")return {available:false,connected:false,source:"fixture",message:"Unavailable in fixture"};
      if(method==="meeting_reminder_status")return {enabled:false,available:false,permission:"unavailable"};
      if(method==="capture_readiness")return {modelReady:options.ready!==false,helperReady:true,microphoneAvailable:true,systemAvailable:false};
      if(method==="create_dictation"){const m={...original,id:"voice-note",kind:"dictation",title:"Voice note",isExample:false,duration:0,status:"draft",notes:"",summary:"",segments:[],messages:[],speakers:[],bookmarks:[],tasks:[],notionUrl:null};meetings.unshift(m);return structuredClone(m);}
      if(method==="start_recording"){if(w.__startFails)throw new Error("Illustrative microphone start failure");const m=meetings.find(m=>m.id===p.meetingId);m.status="recording";status.activeMeetingId=m.id;return structuredClone(m);}
      if(method==="stop_recording"){const m=meetings.find(m=>m.id===p.meetingId);m.status="complete";if(!m.notes)m.notes=m.segments.map((s:any)=>s.text).join(" ");status.activeMeetingId=null;return structuredClone(m);}
      if(method==="update_meeting"){if(w.__saveFails)throw new Error("Illustrative save failure");const m=meetings.find(m=>m.id===p.id);Object.assign(m,p.changes);return structuredClone(m);}
      if(method==="notion_account_status")return {...account};
      if(method==="notion_connect"){w.__notionRequests=(w.__notionRequests||0)+1;account.connecting=true;return new Promise((resolve,reject)=>{resolveConnect={resolve,reject};});}
      if(method==="notion_cancel_connect"){account.connecting=false;resolveConnect?.resolve({...account});return {...account};}
      if(method==="notion_disconnect"){account={...account,connected:false,mode:null};settings.hasNotionToken=false;settings.notionTarget="";settings.notionPropertyMapping={};settings.notionAutoSync=false;return {...account};}
      if(method==="save_secret"){settings.hasNotionToken=true;account={...account,connected:true,mode:"manual"};return null;}
      if(method==="notion_destinations")return {destinations:[{id:"target-test",title:"Team notes",url:"https://notion.so/fixture"}],hasMore:false};
      throw new Error(`Unsupported strict fixture method: ${method}`);
    }};
  }, {original:exampleMeeting(),settings:{...defaults,hasNotionToken:!!options.manual,autoSummary:true,notionAutoSync:!!options.manual},options});
  await page.goto("/");
  await expect(page.getByRole("textbox",{name:"Conversation title"})).toBeVisible();
}
async function notion(page: Page) {await page.getByRole("button",{name:/^Settings/}).click();await page.getByRole("tab",{name:"Notion",exact:true}).click();}
async function connect(page: Page) {
  const previous = await page.evaluate(() => (window as any).__notionRequests || 0);
  await page.getByRole("button", {name:"Connect with Notion"}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__notionRequests || 0)).toBe(previous + 1);
}
const methods = (page: Page) => page.evaluate(() => (window as any).__flowCalls.map((c:any)=>c.method));

test("voice note uses mic only, real speech, editable notes and recoverable copy",async({page})=>{
  await port(page); expect(await methods(page)).not.toContain("start_recording");
  await page.getByRole("button",{name:"Dictate a note"}).click();
  await expect(page.getByRole("button",{name:"Stop dictation"})).toBeVisible();
  await expect(page.getByRole("button",{name:"Computer audio"})).toHaveCount(0);
  await expect(page.getByRole("tab",{name:"Recap",exact:true})).toHaveCount(0);
  await page.evaluate(()=>(window as any).__dictationSegment());
  await expect(page.getByText("Send the updated proposal tomorrow.", {exact:true})).toBeVisible();
  await page.getByRole("button",{name:"Stop dictation"}).click();
  const notes=page.getByRole("textbox",{name:"Voice note text"});await expect(notes).toHaveValue("Send the updated proposal tomorrow.");
  await notes.fill("Edited voice note"); await page.evaluate(()=>(window as any).__copyFails=true);
  await page.getByRole("button",{name:"Copy text"}).click();await expect(page.getByText(/Text could not be copied/)).toBeVisible();
  await expect(notes).toHaveValue("Edited voice note");await page.evaluate(()=>(window as any).__copyFails=false);await page.getByRole("button",{name:"Copy text"}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__copied)).toBe("Edited voice note");
  const calls=await page.evaluate(()=>(window as any).__flowCalls);expect(calls.find((c:any)=>c.method==="capture_readiness").params).toEqual({kind:"dictation"});expect(calls.find((c:any)=>c.method==="start_recording").params).toEqual({meetingId:"voice-note",microphone:true,systemAudio:false});
  expect(await methods(page)).not.toEqual(expect.arrayContaining(["generate_summary"]));expect(await methods(page)).not.toContain("publish_notion");
});
test("dictation setup and active capture guard never create or start a voice note",async({page})=>{await port(page,{ready:false});await page.getByRole("button",{name:"Dictate a note"}).click();await expect(page.getByText("Set up the local speech model and microphone to dictate. Nothing has started.")).toBeVisible();expect(await methods(page)).not.toContain("create_dictation");expect(await methods(page)).not.toContain("start_recording");});
test("another active recording disables dictation",async({page})=>{await port(page,{active:true});await expect(page.getByRole("button",{name:"Dictate a note"})).toBeDisabled();expect(await methods(page)).not.toContain("create_dictation");});
test("failed note save blocks dictation and failed start retains retryable draft",async({page})=>{await port(page);await page.getByRole("tab",{name:"Notes",exact:true}).click();await page.evaluate(()=>(window as any).__saveFails=true);await page.getByRole("textbox",{name:"Meeting notes",exact:true}).fill("Keep my unsaved thought");await page.getByRole("button",{name:"Dictate a note"}).click();await expect(page.getByRole("button",{name:"Retry save"})).toBeVisible();expect(await methods(page)).not.toContain("create_dictation");await page.evaluate(()=>{(window as any).__saveFails=false;(window as any).__startFails=true;});await page.getByRole("button",{name:"Dictate a note"}).click();await expect(page.getByText("Illustrative microphone start failure")).toBeVisible();await expect(page.getByRole("textbox",{name:"Voice note text"})).toBeVisible();await expect(page.getByRole("button",{name:"Start recording",exact:true})).toBeVisible();await page.evaluate(()=>(window as any).__startFails=false);await page.getByRole("button",{name:"Start recording",exact:true}).click();await expect(page.getByRole("button",{name:"Stop dictation"})).toBeVisible();});
test("Notion has honest unconfigured state and an advanced manual migration",async({page})=>{await port(page,{configured:false});await notion(page);await expect(page.getByRole("button",{name:"Connect with Notion"})).toBeDisabled();await expect(page.getByText(/sign-in is not configured/)).toBeVisible();await expect(page.getByLabel("Integration token",{exact:true})).not.toBeVisible();await page.getByText("Advanced connection",{exact:true}).click();await page.getByLabel("Integration token",{exact:true}).fill("fixture-token");await page.getByRole("button",{name:"Save token"}).click();await expect(page.getByRole("heading",{name:"Manual integration connected"})).toBeVisible();await expect(page.getByLabel("Integration token",{exact:true})).toHaveValue("");});
test("Notion browser handoff cancels, recovers from denial and returns to destination picker",async({page})=>{await port(page);await notion(page);await connect(page);await expect(page.getByRole("heading",{name:"Finish connecting in your browser"})).toBeVisible();await page.getByRole("button",{name:"Cancel connection"}).click();await expect(page.getByRole("button",{name:"Connect with Notion"})).toBeEnabled();await connect(page);await page.evaluate(()=>(window as any).__finishNotion(true));await expect(page.getByText(/Authorization was denied/)).toBeVisible();await connect(page);await page.evaluate(()=>(window as any).__finishNotion());await expect(page.getByRole("heading",{name:"Illustrative workspace"})).toBeVisible();await page.getByText("Use an existing database",{exact:true}).click();await page.getByRole("button",{name:"Find",exact:true}).click();await page.getByRole("button",{name:"Team notes"}).click();await expect(page.getByRole("switch",{name:"Automatically publish finished notes"})).toHaveAttribute("aria-checked","false");});
test("Notion return retains unsaved notes and disconnect clears destination opt-in",async({page})=>{await port(page,{manual:true});await page.getByRole("tab",{name:"Notes",exact:true}).click();await page.evaluate(()=>(window as any).__saveFails=true);await page.getByRole("textbox",{name:"Meeting notes",exact:true}).fill("My unsaved local note");await notion(page);await page.getByRole("button",{name:"Disconnect",exact:true}).click();await expect(page.getByRole("switch",{name:"Automatically publish finished notes"})).toHaveAttribute("aria-checked","false");await connect(page);await page.evaluate(()=>(window as any).__finishNotion());await expect(page.getByRole("heading",{name:"Illustrative workspace"})).toBeVisible();await page.getByRole("button",{name:/Planning notes/}).click();await expect(page.getByRole("textbox",{name:"Meeting notes",exact:true})).toHaveValue("My unsaved local note");});

test("bounded Notion and dictation desktop and compact evidence",async({page})=>{
  test.skip(!process.env.TUCK_CAPTURE_CONNECTION, "Run explicitly for the bounded visual review batch.");
  await port(page);
  await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
  const dismissNotice = async () => { const dismiss = page.getByRole("button",{name:"Dismiss notification"}); if(await dismiss.isVisible()) await dismiss.click(); };
  for (const width of [1320,390]) {
    await page.setViewportSize({width,height:900});
    if(width===390) await page.getByRole("button",{name:"Library",exact:true}).click();
    await page.getByRole("button",{name:/Planning notes/}).click();
    await page.getByRole("button",{name:"Open conversation library"}).isVisible().then(async visible=>{if(visible)await page.getByRole("button",{name:"Open conversation library"}).click();});
    await notion(page);
    await dismissNotice();
    await page.screenshot({path:`.impeccable/review/connection-dictation-notion-disconnected-${width}.png`});
    await connect(page);
    await expect(page.getByRole("heading",{name:"Finish connecting in your browser"})).toBeVisible();
    await dismissNotice();
    await page.screenshot({path:`.impeccable/review/connection-dictation-notion-pending-${width}.png`});
    await page.evaluate(()=>(window as any).__finishNotion());
    await expect(page.getByRole("heading",{name:"Illustrative workspace"})).toBeVisible();
    await dismissNotice();
    await page.screenshot({path:`.impeccable/review/connection-dictation-notion-connected-${width}.png`});
    await page.locator(".notion-content-options").scrollIntoViewIfNeeded();
    await page.screenshot({path:`.impeccable/review/connection-dictation-notion-preferences-${width}.png`});
    if(width===390) await page.getByRole("button",{name:"Library",exact:true}).click();
    await page.getByRole("button",{name:"Dictate a note"}).click();
    await expect(page.getByRole("button",{name:"Stop dictation"})).toBeVisible();
    await page.evaluate(()=>(window as any).__dictationSegment());
    await expect(page.getByText("Send the updated proposal tomorrow.",{exact:true})).toBeVisible();
    await dismissNotice();
    await page.screenshot({path:`.impeccable/review/connection-dictation-listening-${width}.png`});
    await page.getByRole("button",{name:"Stop dictation"}).click();
    await expect(page.getByRole("textbox",{name:"Voice note text"})).toHaveValue("Send the updated proposal tomorrow.");
    await dismissNotice();
    await page.screenshot({path:`.impeccable/review/connection-dictation-saved-${width}.png`});
    if(width===1320) {
      await notion(page);await page.getByRole("button",{name:"Disconnect",exact:true}).click();
    }
  }
});

test("Notion section choices persist and prevent publishing with none selected", async ({page}) => {
  await port(page,{manual:true}); await notion(page);
  await expect(page.getByRole("checkbox",{name:"Recap",exact:true})).toBeChecked();
  await page.getByRole("checkbox",{name:"Recap",exact:true}).uncheck();
  await page.getByRole("checkbox",{name:"My notes",exact:true}).uncheck();
  await page.getByRole("checkbox",{name:"Action items",exact:true}).uncheck();
  await expect(page.locator(".notion-content-options").getByText("Choose at least one section before publishing.")).toBeVisible();
  await expect(page.getByRole("switch",{name:"Automatically publish finished notes"})).toBeDisabled();
  const changes=await page.evaluate(()=>(window as any).__flowCalls.filter((c:any)=>c.method==="update_settings").at(-1).params.changes);
  expect(changes.notionContent).toEqual({recap:false,notes:false,tasks:false});
  await page.getByRole("checkbox",{name:"My notes",exact:true}).check();
  await expect(page.locator(".notion-content-options").getByText("Choose at least one section before publishing.")).toHaveCount(0);
});

test("recapture settled disconnected desktop evidence", async ({page}) => {
  test.skip(!process.env.TUCK_CAPTURE_SETTLED_CONNECTION, "Capture validity recovery only.");
  await port(page); await notion(page);
  await expect(page.getByRole("button",{name:"Connect with Notion"})).toBeEnabled();
  await expect(page.getByText(/Your system browser opens for secure sign-in/)).toBeVisible();
  await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
  await page.screenshot({path:".impeccable/review/connection-dictation-notion-disconnected-1320.png"});
});

test("typing during dictation preserves both notes and original speech after stopping", async ({page}) => {
  await port(page); await page.getByRole("button",{name:"Dictate a note"}).click();
  await expect(page.getByRole("button",{name:"Stop dictation"})).toBeVisible();
  await page.evaluate(()=>(window as any).__dictationSegment());
  const notes=page.getByRole("textbox",{name:"Voice note text"});
  await notes.fill("My typed context while speaking");
  await page.getByRole("button",{name:"Stop dictation"}).click();
  await expect(notes).toHaveValue("My typed context while speaking");
  await page.getByText("Original speech",{exact:true}).click();
  await expect(page.getByLabel("Original dictated speech",{exact:true})).toHaveText("Send the updated proposal tomorrow.");
  await page.evaluate(()=>(window as any).__copyFails=true);
  await page.getByRole("button",{name:"Copy original speech",exact:true}).click();
  await expect(page.getByText(/Original speech could not be copied/)).toBeVisible();
  await expect(notes).toHaveValue("My typed context while speaking");
  await page.evaluate(()=>(window as any).__copyFails=false);
  await page.getByRole("button",{name:"Copy original speech",exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__copied)).toBe("Send the updated proposal tomorrow.");
  await notes.fill("My edited final note");
  await page.getByRole("button",{name:"Copy text",exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__copied)).toBe("My edited final note");
  await expect(page.getByLabel("Original dictated speech",{exact:true})).toHaveText("Send the updated proposal tomorrow.");
  expect(await methods(page)).not.toContain("generate_summary"); expect(await methods(page)).not.toContain("publish_notion");
});

test("review confirmation for preserved original speech and publication copy", async ({page}) => {
  test.skip(!process.env.TUCK_CAPTURE_ORIGINAL_SPEECH, "Single functional review confirmation batch.");
  await port(page);
  await page.getByRole("button",{name:"Dictate a note"}).click();
  await expect(page.getByRole("button",{name:"Stop dictation"})).toBeVisible();
  await page.evaluate(()=>(window as any).__dictationSegment());
  await page.getByRole("textbox",{name:"Voice note text"}).fill("My typed context while speaking");
  await page.getByRole("button",{name:"Stop dictation"}).click();
  await expect(page.getByRole("textbox",{name:"Voice note text"})).toHaveValue("My typed context while speaking");
  await page.getByText("Original speech",{exact:true}).click();
  await page.getByRole("button",{name:"Dismiss notification"}).click();
  await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
  for (const width of [1320,390]) {
    await page.setViewportSize({width,height:900});
    await page.screenshot({path:`.impeccable/review/connection-dictation-original-speech-${width}.png`});
  }
  await page.getByRole("button",{name:"Library",exact:true}).click();
  await notion(page);
  await page.getByRole("switch",{name:"Automatically publish finished notes"}).scrollIntoViewIfNeeded();
  await expect(page.getByText(/When a meeting finishes, publish the selected sections/)).toBeVisible();
  await page.screenshot({path:".impeccable/review/connection-dictation-publication-copy-390.png"});
});
