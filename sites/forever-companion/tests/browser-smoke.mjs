import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || '/tmp/forever-companion-browser/node_modules/playwright');
const baseURL = process.env.BASE_URL || 'http://127.0.0.1:4317';
const outputDir = resolve('tests/screenshots');
await mkdir(outputDir, { recursive: true });
const browser = await chromium.launch({headless:true, executablePath:process.env.CHROMIUM_PATH || '/tmp/forever-companion-browser/chromium-native/chrome-linux/chrome',args:['--no-sandbox']});
const errors = [];
const context = await browser.newContext({viewport:{width:1440,height:1000},deviceScaleFactor:1});
const page = await context.newPage();
page.on('pageerror', error => errors.push(String(error)));
const checks = [];
const state = () => page.evaluate(() => JSON.parse(localStorage.getItem('forever-companion-v1')));
async function check(name, body) {await body();checks.push(name);console.log(`PASS ${name}`);}
async function noOverflow(target=page) {assert.ok(await target.evaluate(()=>document.documentElement.scrollWidth<=innerWidth), 'Page must fit viewport width');}
async function nav(view) {await page.locator(`#navigation [data-view="${view}"]`).click();}
try {
  await page.goto(baseURL);
  await page.locator('#recipe-list .recipe-row').first().waitFor();
  await check('desktop shell and real catalog',async()=>{
    await noOverflow();assert.match(await page.title(),/Forever Companion/);
    assert.match(await page.locator('#catalog-count').innerText(),/1,004 recipes/);
    assert.match(await page.locator('#connection').innerText(),/Local sandbox/);
  });
  await page.screenshot({path:resolve(outputDir,'desktop.png'),fullPage:true});
  await check('craft and search filters',async()=>{
    await page.locator('[data-craft="cook"]').click();
    assert.ok(await page.locator('#recipe-list .recipe-row').count()>0);
    assert.match(await page.locator('#recipe-list .recipe-meta').first().innerText(),/Cooking/);
    await page.locator('[data-craft="wood"]').click();
    await page.locator('#recipe-search').fill('Arrowwood Lumber');
    assert.ok(await page.locator('#recipe-list [data-recipe="2"]').count()===1);
    await page.locator('#level-filter').selectOption('10');
    for (const cap of await page.locator('#recipe-list .level').allTextContents()) assert.ok(Number(cap)>=11&&Number(cap)<=20);
    await page.locator('#level-filter').selectOption('all');
    await page.locator('[data-recipe="2"]').click();
    await page.locator('#recipe-search').fill('');
  });
  await check('favorites persist and filter',async()=>{
    await page.locator('#recipe-list [data-favorite="2"]').click();
    await page.locator('#favorite-filter').click();
    assert.equal(await page.locator('#recipe-list .recipe-row').count(),1);
    assert.equal((await state()).favorites[0],2);
    await page.reload();await page.locator('[data-recipe="2"]').waitFor();
    assert.equal(await page.locator('#recipe-list [data-favorite="2"]').getAttribute('aria-pressed'),'true');
  });
  await check('quantity-aware shopping list',async()=>{
    await page.locator('[data-recipe="2"]').click();
    await page.locator('#craft-quantity').fill('7');
    await page.locator('#craft-quantity').press('Tab');
    await page.locator('#add-plan').click();
    assert.equal((await state()).plan[0].quantity,7);
    await nav('planner');
    assert.match(await page.locator('#plan-materials').innerText(),/1 missing/);
    assert.equal(await page.locator('#plan-materials [data-market-item]').count(),2);
  });
  await check('ingredient market bids update inventory and gil',async()=>{
    for(const id of [4098,688]) {
      await nav('planner');
      await page.locator(`#plan-materials [data-market-item="${id}"]`).click();
      assert.equal(await page.locator('#market-list [data-bid]').count(),1);
      await page.locator(`[data-bid="${id}"]`).click();
      assert.equal(await page.locator('#bid-quantity').inputValue(),'1');
      if(id===4098){
        const before=await state(), asking=await page.locator('#bid-price').inputValue();
        await page.locator('#bid-price').fill('1');
        await page.getByRole('button',{name:'Confirm practice bid',exact:true}).click();
        assert.equal(await page.locator('#dialog').isVisible(),true);
        assert.deepEqual(await state(),before);
        assert.match(await page.locator('#toast').innerText(),/unsuccessful/);
        await page.locator('#bid-price').fill(asking);
      }
      await page.getByRole('button',{name:'Confirm practice bid',exact:true}).click();
      assert.equal((await state()).inventory[id],7);
    }
    assert.ok((await state()).gil<50000);
    await nav('planner');
    assert.equal(await page.locator('#plan-materials [data-market-item]').count(),0);
  });
  await check('practice synthesis consumes acquired materials and persists',async()=>{
    await nav('craft');
    const initialLumber=(await state()).inventory[705]||0;
    await page.locator('#practice-craft').click();
    await page.getByRole('button',{name:'Confirm practice synthesis',exact:true}).click();
    const after=await state();
    assert.equal(after.inventory[688],0);assert.equal(after.inventory[4098],0);assert.equal(after.inventory[705],initialLumber+7);
    assert.equal(after.activity.length,3);
    await page.reload();await page.locator('#recipe-list .recipe-row').first().waitFor();
    assert.equal((await state()).inventory[705],initialLumber+7);assert.equal((await state()).gil,after.gil);
    await nav('activity');assert.match(await page.locator('#activity-list').innerText(),/Practice synthesis/);
  });
  await check('inventory and skill edits persist',async()=>{
    await nav('inventory');
    await page.locator('[data-skill="wood"]').fill('15');await page.locator('[data-skill="wood"]').press('Tab');
    await page.locator('#inventory-search').fill('Arrowwood Log');
    await page.locator('[data-stock="688"]').fill('4');await page.locator('[data-stock="688"]').press('Tab');
    assert.equal((await state()).skills.wood,15);assert.equal((await state()).inventory[688],4);
  });
  await check('export downloads actual sandbox',async()=>{
    const event=page.waitForEvent('download');await page.locator('#export').click();const download=await event;
    assert.equal(download.suggestedFilename(),'forever-companion-sandbox.json');assert.equal(await download.failure(),null);
  });
  await check('PWA manifest and offline shell',async()=>{
    const manifest=await page.evaluate(async()=>fetch('manifest.webmanifest').then(r=>r.json()));
    assert.equal(manifest.display,'standalone');assert.equal(manifest.name,'Forever Companion');
    await page.evaluate(()=>navigator.serviceWorker.ready.then(()=>new Promise(resolve=>navigator.serviceWorker.controller?resolve():navigator.serviceWorker.addEventListener('controllerchange',resolve,{once:true}))));
    await context.setOffline(true);await page.reload();
    await page.locator('#skills [data-skill="wood"]').waitFor();
    assert.equal(await page.locator('[data-skill="wood"]').inputValue(),'15');
    await context.setOffline(false);
  });
  const mobileContext=await browser.newContext({viewport:{width:390,height:844},deviceScaleFactor:1,isMobile:true,hasTouch:true});
  const mobile=await mobileContext.newPage();mobile.on('pageerror',e=>errors.push(String(e)));
  await mobile.goto(baseURL);await mobile.locator('#recipe-list .recipe-row').first().waitFor();
  await check('mobile recipe detail and navigation',async()=>{
    await noOverflow(mobile);
    assert.equal(await mobile.locator('#detail').isVisible(),false);
    await mobile.locator('#recipe-list [data-recipe]').last().click();
    await mobile.waitForTimeout(400);
    assert.ok(await mobile.locator('#detail h2').evaluate(e=>e.getBoundingClientRect().top)>=0,'Selected recipe heading must be visible after opening from bottom of long list');
    assert.equal(await mobile.locator('#detail').isVisible(),true);
    await noOverflow(mobile);
    await mobile.screenshot({path:resolve(outputDir,'mobile-detail.png'),fullPage:true});
    await mobile.locator('#back-recipes').click();
    assert.equal(await mobile.locator('#detail').isVisible(),false);
    await mobile.locator('.bottom-nav [data-view="market"]').click();
    await noOverflow(mobile);
    await mobile.screenshot({path:resolve(outputDir,'mobile-market.png'),fullPage:false});
    await mobile.locator('.bottom-nav [data-view="craft"]').click();
    await mobile.screenshot({path:resolve(outputDir,'mobile-log.png'),fullPage:false});
    await mobile.setViewportSize({width:360,height:800});await noOverflow(mobile);
  });
  await mobileContext.close();
  assert.deepEqual(errors,[]);
  await writeFile(resolve(outputDir,'results.json'),JSON.stringify({baseURL,checks,errors},null,2));
  console.log(`PASS ${checks.length} browser checks; no uncaught page errors`);
} finally {await context.close();await browser.close();}
