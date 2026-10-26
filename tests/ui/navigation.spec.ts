import {test,expect} from "@playwright/test";

test.beforeEach(async({page})=>{await page.goto("/");await page.evaluate(()=>localStorage.clear());await page.reload();await page.getByRole("button",{name:"Open an example",exact:true}).click();});

test("destinations, creation actions and saved-item filters have distinct homes",async({page})=>{
  const nav=page.getByRole("navigation",{name:"Main navigation"});
  await expect(nav.getByRole("button")).toHaveCount(2);
  await expect(nav.getByRole("button",{name:/^Library/})).toHaveAttribute("aria-current","page");
  const filters=page.getByRole("group",{name:"Saved item filters"});
  await filters.getByRole("button",{name:/^Starred/}).click();
  await expect(filters.getByRole("button",{name:/^Starred/})).toHaveAttribute("aria-pressed","true");
  await nav.getByRole("button",{name:/^Tasks/}).click();
  await expect(page.getByRole("heading",{name:"Tasks",exact:true})).toBeVisible();
  await expect(page.locator(".meeting-list")).toHaveCount(0);
  await expect(filters).toHaveCount(0);
  await page.locator(".new-menu > summary").click();
  const create=page.getByRole("group",{name:"Create in Tuck"});
  await expect(create.getByRole("button",{name:"New meeting",exact:true})).toBeVisible();
  await expect(create.getByRole("button",{name:"Voice note",exact:true})).toBeVisible();
  await expect(create.getByRole("button",{name:"Import audio…",exact:true})).toBeVisible();
  await page.keyboard.press("Escape"); await expect(create).toBeHidden();
});

test("search combines keywords and Ask Tuck and keeps list filters out of its scope",async({page})=>{
  await page.getByRole("textbox",{name:"Filter saved items"}).fill("No such saved item");
  await expect(page.locator(".meeting-item")).toHaveCount(0);
  await page.getByRole("button",{name:/Search library/}).click();
  await expect(page.getByRole("heading",{name:"Search library",exact:true})).toBeVisible();
  const query=page.getByRole("textbox",{name:"Search or ask your library"});
  await expect(query).toBeFocused();
  await query.fill("custom integration");
  await page.getByRole("button",{name:"Search passages",exact:true}).click();
  await expect(page.locator(".source-result")).toHaveCount(1);
  await expect(page.getByRole("button",{name:"Ask Tuck",exact:true})).toBeVisible();
  await page.getByRole("navigation",{name:"Main navigation"}).getByRole("button",{name:/^Library/}).click();
  await expect(page.getByRole("textbox",{name:"Filter saved items"})).toHaveValue("No such saved item");
  await expect(page.getByRole("textbox",{name:"Conversation title"})).toHaveValue("Northstar / discovery call");
  await page.keyboard.press(process.platform==="darwin"?"Meta+k":"Control+k");
  await expect(query).toBeFocused();
});

test("compact navigation reaches search and Tasks without horizontal overflow",async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await page.getByRole("navigation",{name:"Workspace panes"}).getByRole("button",{name:"Library",exact:true}).click();
  await page.getByRole("button",{name:/Search library/}).click();
  await expect(page.getByRole("textbox",{name:"Search or ask your library"})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);
  await page.getByRole("navigation",{name:"Workspace panes"}).getByRole("button",{name:"Library",exact:true}).click();
  await page.getByRole("navigation",{name:"Main navigation"}).getByRole("button",{name:/^Tasks/}).click();
  await expect(page.getByRole("heading",{name:"Tasks",exact:true})).toBeVisible();
});

test("navigation visual evidence",async({page})=>{
  test.skip(!process.env.TUCK_CAPTURE_NAVIGATION,"Explicit visual capture only");
  for(const width of [1320,390]){
    await page.setViewportSize({width,height:900});
    if(width===390) await page.getByRole("navigation",{name:"Workspace panes"}).getByRole("button",{name:"Library",exact:true}).click();
    await page.getByRole("navigation",{name:"Main navigation"}).getByRole("button",{name:/^Library/}).click();
    await page.screenshot({path:`.impeccable/review/navigation-library-${width}.png`});
    await page.getByRole("button",{name:/Search library/}).click();
    await page.screenshot({path:`.impeccable/review/navigation-search-${width}.png`});
  }
});
