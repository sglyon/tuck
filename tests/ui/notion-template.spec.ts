import { test, expect, type Page } from "@playwright/test";
import { defaults } from "../../src/lib/bridge";
import { exampleMeeting } from "../../src/lib/example";
import { installLeanLibraryPort } from "./support/lean-library";

async function fixture(page: Page, options: {empty?:boolean; searchFails?:boolean; uncertain?:boolean; resumed?:boolean; automatic?:boolean; previousReady?:boolean; readyWarning?:boolean; schemaError?:boolean} = {}) {
  await installLeanLibraryPort(page);
  await page.addInitScript(({settings,meeting,options}) => {
    const w=window as any;
    const calls:any[]=[];w.__templateCalls=calls;w.__remoteCreates=0;
    w.__templateSettings=settings;
    const mapping={date:"Meeting date",folder:"Folder",tags:"Tags",people:"People",meetingId:"Tuck meeting ID"};
    const destination={id:"source-template",databaseId:"database-template",title:"Tuck meetings",url:"https://notion.so/template",properties:[{name:"Name",type:"title"},{name:"Meeting date",type:"date"},{name:"Folder",type:"select"},{name:"Tags",type:"multi_select"},{name:"People",type:"multi_select"},{name:"Tuck meeting ID",type:"rich_text"}],mapping};
    if(options.resumed)settings.notionSetup={parent:"parent-page",status:"uncertain",message:"The database may have been created. Check to continue."};
    if(options.previousReady)settings.notionSetup={parent:"old-parent-page",status:"ready",destination:{id:"old-template",title:"Previous template",url:"https://notion.so/old"}};
    if(options.readyWarning){settings.notionTarget=destination.id;settings.notionPropertyMapping=mapping;settings.notionSetup={parent:"parent-page",status:"ready",destination:{id:destination.id,title:destination.title,url:destination.url},message:"Your database is ready, but the date sort could not be applied. You can set it in Notion."};}
    if(options.schemaError)settings.notionTarget="unavailable-source";
    let uncertain=!!options.uncertain;w.__resolveTemplate=()=>{uncertain=false;};
    const status={engineVersion:"strict illustrative fixture",platform:"test",modelInstalled:true,modelDownloading:false,modelProgress:0,speakerModelInstalled:false,activeMeetingId:null,recordingState:"idle",captureAvailable:true};
    w.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:"main"},currentWebview:{label:"main"}},transformCallback(){return 1;},async invoke(command:string,args:any){
      if(command==="plugin:event|listen")return 1;
      if(command==="plugin:event|unlisten")return;
      if(command!=="engine_call")throw new Error(`Unexpected command ${command}`);
      const {method,params:p={}}=args;calls.push({method,params:p});
      if(method==="bootstrap")return {meetings:[structuredClone(meeting)],settings,accounts:[],status};
      if(method==="get_meeting")return structuredClone(meeting);
      if(method==="get_status")return status;
      if(method==="update_settings"){Object.assign(settings,p.changes);return structuredClone(settings);}
      if(method==="get_meeting_day")return {events:[],snapshots:[],reminders:[],enabled:false,suppressed:false};
      if(method==="calendar_status")return {available:false,connected:false};
      if(method==="meeting_reminder_status")return {enabled:false,available:false,permission:"unavailable"};
      if(method==="notion_account_status")return {configured:true,connected:true,connecting:false,mode:"oauth",workspaceName:"Illustrative workspace",workspaceId:"fixture-workspace"};
      if(method==="notion_parent_pages"){
        if(options.searchFails)throw new Error("Notion could not be reached. Find pages again when your connection returns.");
        return {pages:options.empty?[]:[{id:"parent-page",title:"Tuck",url:"https://notion.so/tuck"},{id:"other-page",title:"Company knowledge",url:"https://notion.so/company"}],hasMore:false};
      }
      if(method==="notion_create_database"){
        if(!settings.notionSetup){w.__remoteCreates++;settings.notionSetup={parent:p.parent,status:"creating"};}
        if(uncertain){settings.notionSetup={parent:p.parent,status:"uncertain",message:"The database may have been created. Check to continue."};throw {code:"notion_creation_uncertain",message:"The database may have been created. Check to continue."};}
        settings.notionTarget=destination.id;settings.notionPropertyMapping=mapping;settings.notionSetup={parent:p.parent,status:"ready",destination:{id:destination.id,title:destination.title,url:destination.url}};
        return {destination,settings:structuredClone(settings)};
      }
      if(method==="notion_destinations")return {destinations:[{id:"existing-source",title:"Existing team meetings",url:"https://notion.so/existing"},{id:"unavailable-source",title:"Unavailable meetings",url:"https://notion.so/unavailable"}],hasMore:false};
      if(method==="notion_destination"){if(settings.notionTarget==="unavailable-source")throw new Error("The selected database could not be read. Check its Notion access.");return {...destination,id:settings.notionTarget,title:settings.notionTarget==="existing-source"?"Existing team meetings":"Tuck meetings"};}
      throw new Error(`Unexpected fixture RPC ${method}`);
    }};
  }, {settings:{...defaults,hasNotionToken:true,notionAutoSync:!!options.automatic},meeting:{...exampleMeeting(),isExample:false,title:"Planning notes",status:"complete"},options});
  await page.goto("/");
  if ((page.viewportSize()?.width || 1320) < 700) await page.getByRole("button",{name:"Library",exact:true}).click();
  await page.getByRole("button",{name:/^Settings/}).click();
  await page.getByRole("tab",{name:"Notion",exact:true}).click();
  await expect(page.getByRole("heading",{name:"Illustrative workspace"})).toBeVisible();
}
async function chooseParent(page: Page) {
  await page.getByRole("button",{name:"Create my meeting database",exact:true}).click();
  await page.getByRole("button",{name:"Tuck",exact:true}).click();
}
const calls=(page:Page)=>page.evaluate(()=>(window as any).__templateCalls);

test("template setup chooses a shared page, creates once and saves complete mappings without publishing",async({page})=>{
  await fixture(page);await chooseParent(page);
  await expect(page.getByText("Meeting name · Meeting date · Folder · Tags · People",{exact:true})).toBeVisible();
  expect((await calls(page)).filter((c:any)=>c.method==="notion_create_database")).toHaveLength(0);
  await page.getByRole("button",{name:"Create in Tuck",exact:true}).click();
  await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
  await expect(page.getByRole("switch",{name:"Automatically publish finished notes"})).toHaveAttribute("aria-checked","false");
  expect(await page.evaluate(()=>(window as any).__remoteCreates)).toBe(1);
  const s=await page.evaluate(()=>(window as any).__templateSettings);
  expect(s.notionTarget).toBe("source-template");expect(s.notionPropertyMapping).toEqual({date:"Meeting date",folder:"Folder",tags:"Tags",people:"People",meetingId:"Tuck meeting ID"});
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("publish_notion");
  await page.getByText("Map meeting details to database columns",{exact:true}).click();
  await expect(page.getByRole("combobox",{name:"Notion meeting date column"})).toHaveValue("Meeting date");
});
test("template setup preserves an existing automatic publishing preference",async({page})=>{
  await fixture(page,{automatic:true});await chooseParent(page);await page.getByRole("button",{name:"Create in Tuck",exact:true}).click();
  await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
  await expect(page.getByRole("switch",{name:"Automatically publish finished notes"})).toHaveAttribute("aria-checked","true");
});
test("a prior template journal never overrides a newly selected parent",async({page})=>{
  await fixture(page,{previousReady:true});await chooseParent(page);
  await page.getByRole("button",{name:"Company knowledge",exact:true}).click();
  await page.getByRole("button",{name:"Create in Company knowledge",exact:true}).click();
  await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
  expect((await calls(page)).find((c:any)=>c.method==="notion_create_database").params.parent).toBe("other-page");
});
test("no shared pages explains access and cannot create a database",async({page})=>{
  await fixture(page,{empty:true});await page.getByRole("button",{name:"Create my meeting database",exact:true}).click();
  await expect(page.getByText(/No shared pages found/)).toBeVisible();
  await expect(page.getByRole("button",{name:"Choose a page to continue"})).toBeDisabled();
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("notion_create_database");
});
test("page search network errors remain retryable and never start setup",async({page})=>{
  await fixture(page,{searchFails:true});await page.getByRole("button",{name:"Create my meeting database",exact:true}).click();
  await expect(page.getByText(/Notion could not be reached/)).toBeVisible();
  await expect(page.getByRole("button",{name:"Find pages",exact:true})).toBeEnabled();
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("notion_create_database");
});
test("an uncertain creation refreshes the journal and only reconciles on check",async({page})=>{
  await fixture(page,{uncertain:true});await chooseParent(page);await page.getByRole("button",{name:"Create in Tuck",exact:true}).click();
  await expect(page.getByRole("heading",{name:"Check your database setup"})).toBeVisible();
  await expect(page.getByRole("button",{name:"Create in Tuck",exact:true})).toHaveCount(0);
  await page.getByRole("button",{name:"Check for my database",exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__remoteCreates)).toBe(1);
  await expect.poll(async ()=>(await calls(page)).filter((c:any)=>c.method==="notion_create_database").length).toBe(2);
  await expect(page.getByRole("button",{name:"Check for my database",exact:true})).toBeEnabled();
  await page.evaluate(()=>(window as any).__resolveTemplate());
  await page.getByRole("button",{name:"Check for my database",exact:true}).click();
  await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__remoteCreates)).toBe(1);
});
test("a resumed uncertain setup offers checking and existing-database recovery",async({page})=>{
  await fixture(page,{resumed:true});await expect(page.getByRole("button",{name:"Check for my database",exact:true})).toBeVisible();
  await page.getByRole("button",{name:"Find existing database",exact:true}).click();
  await page.getByRole("button",{name:"Existing team meetings",exact:true}).click();
  await expect(page.getByText("Notion destination saved.",{exact:true})).toBeVisible();
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("notion_create_database");
});
test("existing database choice remains available without creating a template",async({page})=>{
  await fixture(page);await page.getByText("Use an existing database",{exact:true}).click();
  await page.getByRole("button",{name:"Find",exact:true}).click();await page.getByRole("button",{name:"Existing team meetings",exact:true}).click();
  await expect(page.getByText("Notion destination saved.",{exact:true})).toBeVisible();
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("notion_create_database");
});

test("ready database layout warnings remain visible after reopening settings",async({page})=>{
  await fixture(page,{readyWarning:true});
  await expect(page.getByRole("status").filter({hasText:"date sort could not be applied"})).toBeVisible();
  await page.getByRole("tab",{name:"Intelligence",exact:true}).click();await page.getByRole("tab",{name:"Notion",exact:true}).click();
  await expect(page.getByRole("status").filter({hasText:"date sort could not be applied"})).toBeVisible();
  expect((await calls(page)).map((c:any)=>c.method)).not.toContain("notion_create_database");
});
test("changing to an unreadable database never retains the previous schema mappings",async({page})=>{
  await fixture(page);await page.getByText("Use an existing database",{exact:true}).click();await page.getByRole("button",{name:"Find",exact:true}).click();await page.getByRole("button",{name:"Existing team meetings",exact:true}).click();
  await page.getByText("Map meeting details to database columns",{exact:true}).click();await expect(page.getByRole("combobox",{name:"Notion meeting date column"})).toBeVisible();
  await page.getByText("Use an existing database",{exact:true}).click();await page.getByRole("button",{name:"Find",exact:true}).click();await page.getByRole("button",{name:"Unavailable meetings",exact:true}).click();
  await page.getByText("Map meeting details to database columns",{exact:true}).click();
  await expect(page.getByRole("combobox",{name:"Notion meeting date column"})).toHaveCount(0);
  await expect(page.locator(".notion-mapping").getByRole("alert")).toContainText("selected database could not be read");
});
test("bounded template setup desktop and compact evidence",async({page})=>{
  test.skip(!process.env.TUCK_CAPTURE_NOTION_TEMPLATE,"Run only for the bounded template UI review batch.");
  const widths = process.env.TUCK_CAPTURE_TEMPLATE_WIDTH ? [Number(process.env.TUCK_CAPTURE_TEMPLATE_WIDTH)] : [1320,390];
  for(const width of widths) {
    page = await page.context().newPage();
    await page.setViewportSize({width,height:900});
    await fixture(page);
    await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
    await page.screenshot({path:`.impeccable/review/notion-template-entry-${width}.png`});
    await chooseParent(page);
    await page.locator(".notion-template-setup").scrollIntoViewIfNeeded();
    await page.screenshot({path:`.impeccable/review/notion-template-parent-${width}.png`});
    await page.getByRole("button",{name:"Create in Tuck",exact:true}).click();
    await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
    await page.getByText("Map meeting details to database columns",{exact:true}).click();
    await page.locator(".notion-mapping").scrollIntoViewIfNeeded();
    await page.screenshot({path:`.impeccable/review/notion-template-ready-${width}.png`});
    await page.close();
    page = await page.context().newPage();
    await page.setViewportSize({width,height:900});
    await fixture(page,{resumed:true});
    await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
    await page.locator(".notion-template-setup").scrollIntoViewIfNeeded();
    await page.screenshot({path:`.impeccable/review/notion-template-uncertain-${width}.png`});
    await page.close();
  }
});

test("single settled template mapping confirmation",async({page})=>{
  test.skip(!process.env.TUCK_CONFIRM_NOTION_TEMPLATE,"Single confirmation of settled mapping controls.");
  for(const width of [1320,390]) {
    page = await page.context().newPage();await page.setViewportSize({width,height:900});await fixture(page);await chooseParent(page);
    await page.getByRole("button",{name:"Create in Tuck",exact:true}).click();
    await expect(page.getByText(/Tuck meetings is ready/)).toBeVisible();
    await page.getByRole("button",{name:"Dismiss notification",exact:true}).click();
    await page.getByText("Map meeting details to database columns",{exact:true}).click();
    await page.getByRole("combobox",{name:"Notion tuck meeting id column"}).scrollIntoViewIfNeeded();
    await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
    await page.screenshot({path:`.impeccable/review/notion-template-ready-${width}.png`});await page.close();
  }
});


test("functional confirmation of persisted warnings and current database schema failures",async({page})=>{
  test.skip(!process.env.TUCK_CONFIRM_NOTION_FUNCTIONAL,"Material independent-review correction only.");
  for(const width of [1320,390]) for(const state of ["warning","schema-error"]) {
    page = await page.context().newPage();await page.setViewportSize({width,height:900});await fixture(page,state==="warning"?{readyWarning:true}:{schemaError:true});
    if(state==="warning")await expect(page.getByRole("status").filter({hasText:"date sort could not be applied"})).toBeVisible();
    else {await page.getByText("Map meeting details to database columns",{exact:true}).click();await expect(page.locator(".notion-mapping").getByRole("alert")).toContainText("selected database could not be read");}
    await page.locator(".notion-destination").scrollIntoViewIfNeeded();
    const dismiss=page.getByRole("button",{name:"Dismiss notification",exact:true});if(await dismiss.isVisible())await dismiss.click();
    await page.addStyleTag({content:'body::after{content:"Illustrative UI fixture";position:fixed;right:8px;top:8px;font:10px system-ui;color:#566278;background:#f4f5f8;padding:4px 6px;z-index:9999;border-radius:3px;pointer-events:none}'});
    await page.screenshot({path:`.impeccable/review/notion-template-${state}-${width}.png`});await page.close();
  }
});
