import { test, expect, type Page } from "@playwright/test";
import { defaults } from "../../src/lib/bridge";
import { exampleMeeting } from "../../src/lib/example";
import { installLeanLibraryPort } from "./support/lean-library";

async function fixture(page: Page, options: { permission?: string; shortcut?: boolean; platform?: string; held?: boolean } = {}) {
  await installLeanLibraryPort(page);
  await page.addInitScript(({ original, settings, options }) => {
    const w = window as any;
    const meetings:any[] = [{...original,id:"existing-notes",title:"Illustrative project notes",isExample:false,status:"complete",messages:[]}];
    const calls:any[]=[];w.__dictationCalls=calls;w.__dictationClipboardWrites=0;w.__dictationPasteEvents=0;w.__accessibilityOpened=0;w.__browserClipboardCalls=0;
    Object.defineProperty(navigator,"clipboard",{configurable:true,value:{writeText:async()=>{w.__browserClipboardCalls++;throw new Error("The UI must delegate copying to the native controller");}}});
    let global:any={enabled:true,autoPaste:true,shortcut:"CommandOrControl+Shift+Space",shortcutAvailable:options.shortcut!==false,state:"idle",meetingId:null,pasteAvailable:options.permission==="granted",pastePermission:options.permission || "denied",message:options.shortcut===false?"Another app is using this shortcut. Use the tray menu or start here to copy a voice note.":null};
    const callbacks=new Map<number,any>();const listeners=new Map<string,Set<number>>();let serial=0;
    const emit=(event:string,payload:any)=>{for(const id of listeners.get("engine-event") || [])callbacks.get(id)?.({payload:{event,payload}});};
    w.__recordShortcut=(shortcut:string)=>emit("shortcut_recorded",{shortcut});
    w.__globalStatus=(patch:any)=>{global={...global,...patch};emit("global_dictation_updated",structuredClone(global));};
    w.__globalNativeStart=()=>{global={...global,state:"listening",meetingId:"global-voice",message:null};emit("global_dictation_updated",structuredClone(global));};
    const status={engineVersion:"illustrative global dictation fixture",platform:options.platform || "macos",modelInstalled:true,modelDownloading:false,modelProgress:0,speakerModelInstalled:false,activeMeetingId:null,recordingState:"idle",captureAvailable:true};
    function voice(){let m=meetings.find(m=>m.id==="global-voice");if(!m){m={...original,id:"global-voice",title:"Voice note",kind:"dictation",isExample:false,status:"recording",notes:"",segments:[],messages:[],tasks:[]};meetings.unshift(m);}return m;}
    function finish(){const m=voice();m.status="complete";m.notes="Send the updated proposal after the call.";emit("meeting_updated",{meeting:structuredClone(m)});return m;}
    w.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:"main"},currentWebview:{label:"main"}},transformCallback(cb:any){callbacks.set(++serial,cb);return serial;},async invoke(command:string,args:any){
      if(command==="plugin:event|listen"){if(!listeners.has(args.event))listeners.set(args.event,new Set());listeners.get(args.event)!.add(args.handler);return args.handler;}
      if(command==="plugin:event|unlisten"){listeners.get(args.event)?.delete(args.eventId);return;}
      if(command!=="engine_call")throw new Error(`Unexpected fixture command ${command}`);
      const {method,params:p={}}=args;calls.push({method,params:p});
      if(method==="bootstrap")return {meetings:structuredClone(meetings),settings,accounts:[],status};
      if(method==="get_live_coach"){if(!meetings.some(m=>m.id===p.meetingId))throw new Error("Unknown fixture meeting");return {enabled:false,status:"stopped",selectedAgentIds:[],suggestions:[]};}
      if(method==="get_meeting_day")return {events:[],snapshots:[],reminders:[],enabled:false,suppressed:false};
      if(method==="calendar_status")return {available:false,connected:false};
      if(method==="meeting_reminder_status")return {enabled:false,available:false,permission:"unavailable"};
      if(method==="get_status")return status;
      if(method==="global_dictation_status"){
        const snapshot=structuredClone(global);
        if(options.held && !w.__statusReleased)return new Promise(resolve=>{w.__releaseDictationStatus=()=>{w.__statusReleased=true;resolve(snapshot);};});
        return snapshot;
      }
      if(method==="shortcut_recorder")return {active:p.active};
      if(method==="global_dictation_configure"){
        if(p.shortcut==="Alt+X")throw new Error("That shortcut is unavailable or used by another app. Your previous shortcut is unchanged.");
        global={...global,...p,shortcutAvailable:p.enabled,pasteAvailable:p.autoPaste && options.permission==="granted"};
        emit("global_dictation_updated",structuredClone(global));return structuredClone(global);
      }
      if(method==="global_dictation_toggle"){
        if(w.__startDictationFails){global={...global,state:"error",message:"Your microphone could not start. Check its permission and try again."};emit("global_dictation_updated",structuredClone(global));throw new Error(global.message);}
        if(global.state==="listening"){finish();w.__dictationClipboardWrites++;global={...global,state:"copied",message:null};}
        else{voice();global={...global,state:"listening",meetingId:"global-voice",message:null};}
        emit("global_dictation_updated",structuredClone(global));return structuredClone(global);
      }
      if(method==="global_dictation_cancel"){finish();global={...global,state:"idle",message:"Voice note saved. Nothing was copied or inserted."};emit("global_dictation_updated",structuredClone(global));return structuredClone(global);}
      if(method==="dictation_accessibility_settings"){w.__accessibilityOpened++;if(w.__permissionOpenFails)throw new Error("System Settings could not open. Open Privacy & Security → Accessibility yourself.");return {opened:true};}
      if(method==="meeting_audio")return [];
      throw new Error(`Unexpected global dictation fixture RPC: ${method}`);
    }};
  },{original:exampleMeeting(),settings:defaults,options});
  await page.goto("/");await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Illustrative project notes");
}
const region=(page:Page)=>page.getByRole("region",{name:"Dictation anywhere",exact:true});
const indicator=(page:Page)=>page.getByRole("complementary",{name:"Global dictation status",exact:true});
const calls=(page:Page)=>page.evaluate(()=>(window as any).__dictationCalls);
async function settings(page:Page){if((page.viewportSize()?.width || 1320)<700)await page.getByRole("button",{name:"Library",exact:true}).click();await page.getByRole("button",{name:/^Settings/}).click();await page.getByRole("tab",{name:"Audio & models",exact:true}).click();await expect(region(page).getByText("Not recording",{exact:true})).toBeVisible();}

test("global dictation never starts from opening settings and AX access needs an explicit click",async({page})=>{
  await fixture(page);expect((await calls(page)).filter((c:any)=>c.method==="global_dictation_status")).toHaveLength(0);await settings(page);await expect(region(page).getByText("Microphone only. Local transcription. No AI rewriting.")).toBeVisible();expect((await calls(page)).filter((c:any)=>c.method==="global_dictation_toggle")).toHaveLength(0);expect(await page.evaluate(()=>(window as any).__accessibilityOpened)).toBe(0);await region(page).getByRole("button",{name:"Open Accessibility settings"}).click();await expect.poll(()=>page.evaluate(()=>(window as any).__accessibilityOpened)).toBe(1);
});
test("starting in Tuck keeps the current document and saves without any browser clipboard or paste call",async({page})=>{
  await fixture(page);await settings(page);await region(page).getByRole("button",{name:"Start dictation here"}).click();await expect(indicator(page)).toContainText("Listening · microphone only");await region(page).getByRole("button",{name:"Finish dictation"}).click();await expect(indicator(page)).toHaveCount(0);await expect(region(page).getByText("Copied. Paste your words wherever you need them.",{exact:true})).toBeVisible();expect(await page.evaluate(()=>(window as any).__dictationClipboardWrites)).toBe(1);expect(await page.evaluate(()=>(window as any).__dictationPasteEvents)).toBe(0);await region(page).getByRole("button",{name:"Open voice note"}).click();await expect(page.getByRole("textbox",{name:"Voice note text"})).toHaveValue("Send the updated proposal after the call.");expect(await page.evaluate(()=>(window as any).__browserClipboardCalls)).toBe(0);for(const forbidden of ["start_recording","create_dictation","summarize","publish_notion"])expect((await calls(page)).map((c:any)=>c.method)).not.toContain(forbidden);
});
test("save without inserting stops safely without copying or claiming a paste",async({page})=>{
  await fixture(page);await settings(page);await region(page).getByRole("button",{name:"Start dictation here"}).click();await region(page).getByRole("button",{name:"Save without inserting"}).click();await expect(region(page).getByText("Voice note saved. Nothing was copied or inserted.",{exact:true})).toBeVisible();expect(await page.evaluate(()=>(window as any).__dictationClipboardWrites)).toBe(0);expect(await page.evaluate(()=>(window as any).__dictationPasteEvents)).toBe(0);await region(page).getByRole("button",{name:"Open voice note"}).click();await expect(page.getByRole("textbox",{name:"Voice note text"})).toHaveValue("Send the updated proposal after the call.");
});
test("shortcut events stay quiet and never change the selected meeting",async({page})=>{
  await fixture(page);await page.evaluate(()=>(window as any).__globalNativeStart());await expect(indicator(page)).toBeVisible();await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Illustrative project notes");await page.evaluate(()=>(window as any).__globalStatus({state:"processing"}));await expect(indicator(page)).toContainText("Finishing your voice note…");await expect(indicator(page).getByRole("button",{name:"Save without inserting"})).toBeVisible();await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Illustrative project notes");
});
test("a stale status read cannot replace a newer native listening event",async({page})=>{
  await fixture(page,{held:true});await page.getByRole("button",{name:/^Settings/}).click();await page.getByRole("tab",{name:"Audio & models",exact:true}).click();await expect.poll(()=>page.evaluate(()=>!!(window as any).__releaseDictationStatus)).toBe(true);await page.evaluate(()=>{(window as any).__globalNativeStart();(window as any).__releaseDictationStatus();});await expect(region(page).getByText("Listening · microphone only",{exact:true})).toBeVisible();await expect(region(page).getByRole("button",{name:"Finish dictation"})).toBeEnabled();
});
test("permissions and shortcut conflicts preserve clipboard-only dictation",async({page})=>{
  await fixture(page,{platform:"linux-wayland",permission:"unavailable",shortcut:false});await settings(page);await expect(region(page).getByText(/Another app is using this shortcut/)).toBeVisible();await expect(region(page).getByRole("button",{name:"Open Accessibility settings"})).toHaveCount(0);await expect(region(page).getByText("Your words are copied to the clipboard. Paste them yourself when automatic insertion is unavailable.",{exact:true})).toBeVisible();await expect(region(page).getByRole("button",{name:"Start dictation here"})).toBeEnabled();await region(page).getByRole("button",{name:"Start dictation here"}).click();await expect(indicator(page)).toBeVisible();
});
test("native failures are actionable and a paste dispatch is described without claiming insertion",async({page})=>{
  await fixture(page,{permission:"granted"});await settings(page);await page.evaluate(()=>(window as any).__startDictationFails=true);await region(page).getByRole("button",{name:"Start dictation here"}).click();await expect(indicator(page)).toContainText("Your microphone could not start.");await indicator(page).getByRole("button",{name:"Dismiss dictation status"}).click();await expect(indicator(page)).toHaveCount(0);await page.evaluate(()=>(window as any).__globalStatus({state:"paste_sent",message:null,meetingId:"global-voice"}));await expect(region(page).getByText("Copied. Paste sent to your text field.",{exact:true})).toBeVisible();await expect(region(page).getByText(/inserted successfully/i)).toHaveCount(0);
});

test("one stable polite announcer reports all global phases outside Settings without moving focus",async({page})=>{
  await fixture(page);
  const announcer=page.getByRole("status",{name:"Dictation announcements",exact:true});
  await expect(announcer).toHaveCount(1);await expect(announcer).toHaveAttribute("aria-live","polite");await expect(announcer).toHaveAttribute("aria-atomic","true");
  await page.getByRole("textbox",{name:"Conversation title"}).focus();
  await page.evaluate(()=>{(window as any).__announcerNode=document.querySelector('[aria-label="Dictation announcements"]');});
  for(const [state,text] of [["starting","Getting your microphone ready…"],["listening","Listening · microphone only"],["processing","Finishing your voice note…"],["copied","Copied. Paste your words wherever you need them."],["paste_sent","Copied. Paste sent to your text field."],["error","Dictation needs attention"]]){
    await page.evaluate(state=>(window as any).__globalStatus({state,message:null}),state);await expect(announcer).toHaveText(text);await expect(announcer).toHaveCount(1);await expect(page.getByRole("textbox",{name:"Conversation title"})).toBeFocused();expect(await page.evaluate(()=>(window as any).__announcerNode===document.querySelector('[aria-label="Dictation announcements"]'))).toBe(true);
  }
  await page.getByRole("button",{name:/^Settings/}).click();await page.getByRole("tab",{name:"Audio & models",exact:true}).click();await page.evaluate(()=>(window as any).__globalStatus({state:"copied",message:null}));await expect(announcer).toHaveText("Copied. Paste your words wherever you need them.");await expect(region(page).locator('[role="status"]:not(#dictation-shortcut-help), [aria-live]')).toHaveCount(0);expect(await page.evaluate(()=>(window as any).__announcerNode===document.querySelector('[aria-label="Dictation announcements"]'))).toBe(true);
});

test("bounded global dictation desktop and compact evidence",async({page})=>{
  test.skip(!process.env.TUCK_CAPTURE_GLOBAL_DICTATION,"Run explicitly for one bounded visual batch.");await fixture(page,{permission:"granted"});await settings(page);await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;pointer-events:none}'});
  for(const width of [1320,390]){
    await page.setViewportSize({width,height:900});await region(page).scrollIntoViewIfNeeded();
    await page.evaluate(()=>(window as any).__globalStatus({state:"idle",shortcutAvailable:true,pasteAvailable:true,pastePermission:"granted",meetingId:null,message:null}));await expect(region(page).getByText("Not recording",{exact:true})).toBeVisible();await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-ready-${width}.png`});
    await page.evaluate(()=>(window as any).__globalStatus({pasteAvailable:false,pastePermission:"denied"}));await expect(region(page).getByRole("button",{name:"Open Accessibility settings"})).toBeVisible();await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-permission-${width}.png`});
    await region(page).getByRole("button",{name:"Start dictation here"}).click();await expect(indicator(page)).toBeVisible();await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-listening-${width}.png`});
    await region(page).getByRole("button",{name:"Finish dictation"}).click();await expect(region(page).getByText("Copied. Paste your words wherever you need them.",{exact:true})).toBeVisible();await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-saved-${width}.png`});
    await page.evaluate(()=>(window as any).__globalStatus({state:"error",message:"Your microphone could not start. Check its permission and try again."}));await expect(indicator(page)).toBeVisible();await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-error-${width}.png`});
    await page.evaluate(()=>(window as any).__globalStatus({state:"idle",shortcutAvailable:false,pasteAvailable:false,pastePermission:"unavailable",message:"This shortcut is unavailable in this desktop session. Start here or use Tuck’s tray menu; your words are copied for manual paste."}));await expect(indicator(page)).toHaveCount(0);await region(page).locator(".section-heading").scrollIntoViewIfNeeded();await page.screenshot({path:`.impeccable/review/global-dictation-confirm-shortcut-${width}.png`});
  }
});

test("dictation preferences save, disable capture, restore defaults and preserve conflicts",async({page})=>{
  await fixture(page,{permission:"granted"}); await settings(page);
  const area=region(page);
  await recordShortcut(page,"Alt+Shift+KeyD");
  await area.getByRole("switch",{name:/Paste automatically/}).uncheck();
  await area.getByRole("button",{name:"Save dictation settings"}).click();
  await expect(area.getByText("Dictation settings saved on this device.")).toBeVisible();
  await expect(area.locator("kbd")).toHaveText(/Option \+ Shift \+ D|Alt \+ Shift \+ D/);
  expect(await page.evaluate(()=>(window as any).__dictationCalls.filter((c:any)=>c.method==="global_dictation_configure").at(-1).params)).toEqual({enabled:true,shortcut:"Alt+Shift+D",autoPaste:false});
  await area.getByRole("switch",{name:/Enable dictation anywhere/}).uncheck();
  await area.getByRole("button",{name:"Save dictation settings"}).click();
  await expect(area.getByRole("button",{name:"Start dictation here"})).toBeDisabled();
  await expect(area.getByText("Dictation anywhere is off. Its global shortcut is released.")).toBeVisible();
  await area.getByRole("button",{name:"Restore defaults"}).click();
  await area.getByRole("button",{name:"Save dictation settings"}).click();
  await expect(area.getByRole("button",{name:"Start dictation here"})).toBeEnabled();
  await recordShortcut(page,"Alt+KeyX");
  await area.getByRole("button",{name:"Save dictation settings"}).click();
  await expect(area.getByText(/That shortcut is unavailable or used by another app/)).toBeVisible();
  await expect(area.locator("kbd")).toHaveText(/Shift \+ Space/);
  await area.getByRole("button",{name:"Start dictation here"}).click();
  await expect(area.getByRole("button",{name:"Record shortcut"})).toBeDisabled();
  await expect(area.getByText("Finish dictation before changing these settings.")).toBeVisible();
});

async function recordShortcut(page: Page, chord: string) {
  await region(page).getByRole("button",{name:"Record shortcut",exact:true}).click();
  await expect(region(page).getByText("Press your shortcut now. Escape cancels.")).toBeVisible();
  await page.keyboard.press(chord);
}

test("shortcut recorder cancels, rejects unmodified keys and captures app shortcuts without navigating",async({page})=>{
  await fixture(page); await settings(page);
  const area=region(page);
  await recordShortcut(page,"KeyD");
  await expect(area.getByText("Use a function key on its own, or add Command, Control, or Option/Alt.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(area.getByText("Shortcut recording canceled.")).toBeVisible();
  await expect(area.getByRole("status").filter({hasText:"Shortcut recording canceled"})).toBeVisible();
  await recordShortcut(page,"Control+KeyK");
  await expect(area).toBeVisible();
  await expect(area.getByRole("status").filter({hasText:"Shortcut captured"})).toBeVisible();
  await expect(area.locator("output")).toHaveText("Ctrl + K");
  expect(await page.evaluate(()=>(window as any).__dictationCalls.filter((c:any)=>c.method==="global_dictation_configure"))).toEqual([]);
});

test("recorder handles registered native shortcuts and cancels on blur",async({page})=>{
  await fixture(page); await settings(page);
  const area=region(page);
  await recordShortcut(page,"Shift");
  await page.evaluate(()=>(window as any).__recordShortcut("super+shift+KeyR"));
  await expect(area.getByText("That shortcut starts a meeting. Choose another shortcut for dictation.")).toBeVisible();
  await page.evaluate(()=>(window as any).__recordShortcut("super+shift+Space"));
  await expect(area.getByText("Shortcut captured. Save dictation settings to apply it.")).toBeVisible();
  await expect(area.getByRole("button",{name:"Start dictation here"})).toBeEnabled();
  await recordShortcut(page,"Shift");
  await area.getByRole("switch",{name:/Paste automatically/}).focus();
  await expect(area.getByRole("button",{name:"Record shortcut",exact:true})).toBeVisible();
  await expect.poll(async()=>page.evaluate(()=>(window as any).__dictationCalls.filter((c:any)=>c.method==="shortcut_recorder").at(-1)?.params.active)).toBe(false);
  expect(await page.evaluate(()=>(window as any).__dictationCalls.filter((c:any)=>c.method==="global_dictation_toggle"))).toEqual([]);
});

test("F20 can be recorded and saved without a modifier",async({page})=>{
  await fixture(page); await settings(page);
  const area=region(page);
  await area.getByRole("button",{name:"Record shortcut",exact:true}).click();
  await expect(area.getByText("Press your shortcut now. Escape cancels.")).toBeVisible();
  // Playwright's keyboard table stops at F12; dispatch the extended function key.
  await page.evaluate(()=>window.dispatchEvent(new KeyboardEvent("keydown",{key:"F20",code:"F20",bubbles:true,cancelable:true})));
  await expect(area.locator("output")).toHaveText("F20");
  await area.getByRole("button",{name:"Save dictation settings"}).click();
  await expect(area.getByText("Dictation settings saved on this device.")).toBeVisible();
  await expect(area.locator("kbd")).toHaveText("F20");
  expect(await page.evaluate(()=>(window as any).__dictationCalls.filter((c:any)=>c.method==="global_dictation_configure").at(-1).params.shortcut)).toBe("F20");
});
