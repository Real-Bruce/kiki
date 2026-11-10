// Documentation-only vector sketches. Not imported by the Rust application.
// Run: node docs/design/next/render-sketches.cjs [--svg-only]
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { pathToFileURL } = require('node:url');
const out = __dirname;
const W = 1280, H = 900;
const palettes = {
  light: { bg:'#EEEBE5', panel:'#FAF9F6', side:'#F0EDE7', text:'#252722', muted:'#696D65', border:'#D7D5CC', accent:'#365F56', active:'#E2EBE5', paper:'#FFFFFF', faint:'#E6E6DF', red:'#FF3B40' },
  dark: { bg:'#1A1C19', panel:'#262824', side:'#2E302B', text:'#F3F1EA', muted:'#B6BAB0', border:'#484C43', accent:'#A0CEBB', active:'#354B40', paper:'#22251F', faint:'#42473D', red:'#FF3B40' }
};
let c, body;
const esc = s => String(s).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');
function add(s){ body.push(s); }
function rect(x,y,w,h,fill=c.panel,stroke='none',r=0,sw=1){add(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${r}" fill="${fill}" stroke="${stroke}" stroke-width="${sw}"/>`);}
function line(x1,y1,x2,y2,color=c.border,w=1,dash=''){add(`<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${color}" stroke-width="${w}" ${dash?`stroke-dasharray="${dash}"`:''} stroke-linecap="round"/>`);}
function circle(x,y,r,fill,stroke='none',sw=1){add(`<circle cx="${x}" cy="${y}" r="${r}" fill="${fill}" stroke="${stroke}" stroke-width="${sw}"/>`);}
function txt(x,y,s,size=18,color=c.text,weight=400,anchor='start'){add(`<text x="${x}" y="${y}" font-size="${size}" fill="${color}" font-weight="${weight}" text-anchor="${anchor}">${esc(s)}</text>`);}
function lines(x,y,ss,size=18,color=c.muted,gap=30){ss.forEach((s,i)=>txt(x,y+i*gap,s,size,color));}
function note(x,y,head,ss){txt(x,y,head,21,c.accent,600);lines(x,y+35,ss,17,c.muted,29);}
function pathD(d,color=c.text,w=2,fill='none'){add(`<path d="${d}" fill="${fill}" stroke="${color}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round"/>`);}
function icon(name,x,y,color=c.text){
 add(`<g transform="translate(${x} ${y})">`);
 const ds={rect:'M3 3H25V25H3Z',arrow:'M3 25L25 3M10 3H25V18',pen:'M3 25C8 11 9 2 15 3C23 4 5 23 17 21C21 19 24 16 25 18',text:'M3 4H25M14 4V25M9 25H19',pin:'M8 3L25 20M8 3L4 7L9 12L6 19L11 22L16 16L21 21L25 20M9 21L3 27',save:'M4 3H22L26 7V26H3V3H4M8 3V12H21V3M8 26V17H21V26',copy:'M10 3H26V21H10ZM5 8H3V27H20V24',close:'M5 5L23 23M23 5L5 23',long:'M5 8V3H11M20 3H25V8M5 21V26H11M20 26H25V21M15 7V22M11 18L15 22L19 18',picker:'M19 3L26 10L21 15L14 8ZM16 11L5 22V26H9L20 15M3 28L5 26',more:'M5 14H5M14 14H14M23 14H23'};
 if(name==='mosaic'){for(let i=0;i<3;i++)for(let j=0;j<3;j++)rect(3+i*8,3+j*8,6,6,(i+j)%2?color:'none',color,0,1);}
 else if(name==='number'){circle(14,14,12,'none',color,2);txt(14,20,'1',18,color,500,'middle');}
 else if(name==='more'){[5,14,23].forEach(a=>circle(a,14,2,color));}
 else pathD(ds[name]||ds.rect,color,1.8);
 add('</g>');
}
const baseTools=['rect','arrow','pen','text'];
const allTools=['rect','arrow','pen','text','mosaic','number','long','picker'];
function toolbar(x,y,tools=baseTools,selected='rect'){
 const items=[...tools,'pin','save','copy','close'];
 const sep=tools.length>6?3:2;
 const w=items.length*48+24+sep*12;
 rect(x,y,w,58,c.panel,c.border,12);
 let px=x+12;
 items.forEach((it,i)=>{
   if(i===tools.length||i===items.length-1||(tools.length>6&&i===6)){line(px+4,y+15,px+4,y+43);px+=12;}
   if(it===selected)rect(px,y+7,42,44,c.active,'none',7);
   icon(it,px+7,y+15,it===selected?c.accent:c.text);px+=48;
 });
 return w;
}
function handles(x,y,w,h,accent=c.accent){[[x,y],[x+w/2,y],[x+w,y],[x,y+h/2],[x+w,y+h/2],[x,y+h],[x+w/2,y+h],[x+w,y+h]].forEach(([a,b])=>rect(a-4,b-4,8,8,accent,c.panel,0));}
function selection(x,y,w,h,adjustable=true){rect(x,y,w,h,c.paper,c.accent,0,1.5);if(adjustable)handles(x,y,w,h);rect(x,y-32,145,26,c.accent,'none',5);txt(x+12,y-13,`${w} × ${h} px`,15,c.panel);}
function slider(x,y,label,value,w=210){txt(x,y+6,label,17);line(x+110,y,x+110+w,y,c.border,4);const ranges={'字号':[8,256],'文字字号':[8,256],'圆角半径':[0,256],'颗粒大小':[2,64],'直径':[16,128]};const [min,max]=ranges[label]||[1,64];const knob=x+110+w*Math.max(0,Math.min(1,(value-min)/(max-min)));line(x+110,y,knob,y,c.accent,4);circle(knob,y,7,c.accent);rect(x+130+w,y-17,66,34,c.panel,c.border,6);txt(x+163+w,y+6,String(value),17,c.text,400,'middle');txt(x+208+w,y+6,'px',15,c.muted);}
function colors(x,y){['#FF3B40','#FF9800','#FFC900','#28BC4C','#1684F8','#A254DE','#FFFFFF','#171815'].forEach((col,i)=>circle(x+i*32,y,9,col,c.border,1));circle(x,y,13,'none',c.accent,1.5);}
function param(x,y,label,value){rect(x,y,630,56,c.panel,c.border,10);colors(x+24,y+28);line(x+269,y+13,x+269,y+43);slider(x+286,y+28,label,value,110);}
function button(x,y,w,label,primary=false){rect(x,y,w,38,primary?c.active:c.panel,primary?c.accent:c.border,7);txt(x+w/2,y+25,label,16,primary?c.accent:c.text,500,'middle');}
function toggle(x,y,on){rect(x,y,38,21,on?c.accent:c.muted,'none',11);circle(x+(on?27:11),y+10.5,7.5,c.panel);}
function numberMark(x,y,n,r=20){circle(x,y,r,'none',c.red,2.5);txt(x,y+r*0.34,String(n),r*1.12,c.red,500,'middle');}
function content(x,y,w,count=5){for(let i=0;i<count;i++){rect(x,y+i*40,w-(i%3)*40,9,c.faint,'none',4);}}
function scene(title,sub,theme,fn){c=palettes[theme];body=[];rect(0,0,W,H,c.bg);txt(40,55,title,30,c.text,600);txt(40,91,sub,17,c.muted);rect(1060,28,180,32,c.active,'none',16);txt(1150,50,theme==='light'?'浅色 · 设计草图':'深色 · 设计草图',15,c.accent,500,'middle');fn();line(40,825,1240,825);txt(40,859,'kiki · 2026-11-10 · 工具条为 F12 终态，过渡版本保留旧入口',15,c.muted);txt(1240,859,'非应用实机截图 · 演示内容不进入产品',15,c.muted,400,'end');return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="900" viewBox="0 0 1280 900" role="img" aria-label="${esc(title)}"><title>${esc(title)}</title><defs><pattern id="checker" width="20" height="20" patternUnits="userSpaceOnUse"><rect width="20" height="20" fill="#D4D8D1"/><path d="M0 0H10V10H0ZM10 10H20V20H10Z" fill="#F2F2ED"/></pattern></defs><g font-family="Segoe UI, Microsoft YaHei, sans-serif">${body.join('\n')}</g></svg>\n`;}
const scenes=[
 ['s01-edit','S01  直接编辑，无需切换选择工具','F01 · 选区边线始终可调整；同类对象优先命中',()=>{
  selection(55,185,800,430);txt(95,235,'项目说明',27,c.text,600);content(95,268,670,2);
  rect(100,374,240,100,'none',c.red,0,3);handles(100,374,240,100);txt(120,432,'拖轮廓移动矩形',20,c.red);
  line(415,473,570,374,c.red,4);pathD('M570 374L542 378L557 401Z',c.red,1,c.red);circle(415,473,5,c.accent);circle(570,374,5,c.accent);
  rect(580,500,220,57,'none',c.accent,0);txt(596,537,'点击编辑文字',24,c.red);line(690,500,690,479,c.accent);circle(690,473,6,c.panel,c.accent,2);
  toolbar(130,642,baseTools,'text');
  note(900,205,'矩形 / 箭头 / 画笔',['点击同类对象即可操作','轮廓或笔迹拖动＝移动','矩形控制点＝调整大小','箭头两端＝长度和方向']);
  note(900,390,'文字',['单击内容＝重新编辑','拖边框＝移动','拖顶部圆点＝旋转']);
  note(900,545,'截图边界',['四条边及八点可调整','不依赖当前绘图工具','空格＋拖动移动选区']);
  txt(80,760,'对象选中、输入状态、命中热区必须真实实现；一次连续操作对应一次撤销。',18,c.muted);
 }],
 ['s02-parameters','S02  连续参数与滚轮调节','F02 / F03 · 实际成图像素；各工具独立记忆；与设置双向同步',()=>{
  rect(45,140,1190,610,c.panel,c.border,14);
  txt(80,185,'矩形 / 箭头 / 画笔',24,c.text,600);toolbar(80,209,baseTools,'arrow');param(80,281,'线宽',3);
  line(95,381,610,381,c.red,3);txt(735,242,'1–64 px',25,c.accent,600);lines(735,282,['默认 3 px；滚轮每格 ±1 px','绘制中立即改变当前对象整体线宽','选中对象时同时更新对象与默认值'],18,c.muted,34);
  line(80,420,1195,420);txt(80,467,'文字',24,c.text,600);toolbar(80,492,baseTools,'text');param(80,563,'字号',24);txt(100,665,'透明文字编辑框保留中文输入',24,c.red);
  txt(735,525,'8–256 px',25,c.accent,600);lines(735,565,['默认 24 px；滚轮每格 ±1 px','输入数值、滑杆、滚轮使用同一参数','重新截图与重启后保持'],18,c.muted,34);
  txt(65,790,'仅选中旧对象不覆盖默认值；连续滚轮合并撤销记录与后台配置写入。',18,c.muted);
 }],
 ['s03-settings','S03  截图设置与输出','F02–F07 · 保持六页导航与无卡片分组；新增工具默认关闭',()=>{
  rect(40,135,770,660,c.panel,c.border,12);rect(41,136,170,658,c.side,'none',11);txt(68,184,'kiki',27,c.accent,600);
  ['通用','快捷键','截图','贴图','外观','关于'].forEach((a,i)=>{if(i===2)rect(54,289,141,41,c.active,'none',7);txt(77,235+i*40,a,18,i===2?c.accent:c.text);});
  txt(239,181,'截图',27,c.text,600);txt(239,208,'中段视图 · 上方保留操作提示与颜色，尺寸为实际 px',14,c.muted);rect(795,225,4,465,c.border,'none',2);rect(795,343,4,172,c.accent,'none',2);
  ['矩形线宽','箭头线宽','画笔线宽','文字字号'].forEach((a,i)=>slider(240,244+i*41,a,i===3?24:3,210));
  line(239,394,778,394);slider(240,427,'圆角半径',0,210);txt(240,479,'输出格式',17);button(350,454,118,'PNG   ⌄');txt(486,479,'可选择 JPEG',15,c.muted);
  line(239,503,778,503);txt(240,533,'工具显示',20,c.text,600);
  ['矩形','箭头','画笔','文字','长截图','马赛克','编号','取色'].forEach((a,i)=>{const col=i%2,row=Math.floor(i/2);txt(240+col*270,570+row*36,a,16);toggle(432+col*270,553+row*36,i<4);});
  button(239,711,184,'恢复截图默认设置');txt(239,774,'✓ 设置已保存',15,c.accent);
  txt(845,174,'圆角效果对照（示例 30 px）',20,c.text,600);rect(845,198,170,105,'url(#checker)',c.border,0);rect(845,198,170,105,c.accent,'none',30);txt(930,260,'PNG',22,c.panel,600,'middle');txt(845,328,'四角透明',16,c.muted);
  rect(1040,198,170,105,'#FFFFFF',c.border,0);rect(1040,198,170,105,c.accent,'none',30);txt(1125,260,'JPEG',22,c.panel,600,'middle');txt(1040,328,'四角白底',16,c.muted);
  txt(845,370,'半径 0–256 px，默认 0',17,c.muted);txt(845,399,'JPEG 品质 90%；默认输出 PNG',17,c.muted);
  rect(840,450,390,284,c.panel,c.border,12);txt(864,491,'恢复截图默认设置',22,c.text,600);lines(864,532,['恢复标注参数、圆角、输出格式、','操作提示及工具显示。','热键、主题、自启和保存目录','保持不变。'],16,c.muted,28);button(995,676,84,'取消');button(1091,676,113,'恢复默认',true);
  txt(845,774,'启用新工具后，展开其默认参数。',16,c.muted);
 }],
 ['s04-mosaic','S04  可编辑的矩形马赛克','F08 · 拖控制点调整覆盖区域；滚轮仅调整颗粒',()=>{
  selection(55,185,790,420);txt(100,240,'联系资料',27,c.text,600);txt(100,296,'姓名',18,c.muted);txt(260,296,'示例用户',21);txt(100,356,'联系方式',18,c.muted);
  const shades=[c.faint,c.border,c.muted,c.side];for(let j=0;j<5;j++)for(let i=0;i<20;i++)rect(255+i*22,329+j*22,22,22,shades[(i*7+j*3)%4]);
  rect(255,329,440,110,'none',c.accent,0,1.5);handles(255,329,440,110);txt(100,500,'移动后，原位置恢复冻结底图。',20,c.muted);
  toolbar(90,634,['rect','arrow','pen','text','mosaic'],'mosaic');rect(150,708,550,56,c.panel,c.border,10);slider(172,736,'颗粒大小',12,245);
  note(900,220,'区域操作',['拖动创建矩形','拖内部移动区域','拖八个控制点缩放','Delete 删除，Ctrl+Z 撤销']);
  note(900,430,'颗粒参数',['默认 12 px；范围 2–64 px','滚轮每格 ±1 px','参数与设置同步','只处理底图，标注绘制于上层']);
 }],
 ['s05-number','S05  连续顺序标识','F09 · 红色圆环与红色数字，透明中心；按创建顺序自动重排',()=>{
  selection(55,185,790,420);numberMark(130,280,1,25);txt(180,288,'打开设置',25);numberMark(350,385,2,25);txt(400,393,'配置工具',25);numberMark(570,490,3,25);txt(620,498,'完成截图',25);
  toolbar(90,634,['rect','arrow','pen','text','number'],'number');rect(140,708,620,56,c.panel,c.border,10);txt(160,743,'下一编号 ④',17);slider(305,736,'直径',28,190);
  note(900,220,'插入与调整',['单击插入下一个编号','拖已有编号移动','滚轮每格 ±2 px','直径 16–128 px，默认 28']);
  note(900,415,'删除中间编号',['删除②后，原③变②','撤销后恢复原顺序','每次新截图从①开始']);
  numberMark(940,635,9,22);numberMark(1020,635,10,22);numberMark(1100,635,11,22);txt(900,695,'数字真实绘制，不依赖圈号字形。',16,c.muted);
 }],
 ['s06-picker','S06  屏幕取色','F10 · 单击复制并留在截图；每按一次 Shift 持续切换格式',()=>{
  selection(55,185,790,420);rect(115,250,220,220,'#365F56','none',15);rect(355,250,190,220,'#A0CEBB','none',15);rect(565,250,220,220,'#E4D9C4','none',15);
  line(200,330,230,330,'#FFFFFF',1.5);line(215,315,215,345,'#FFFFFF',1.5);rect(237,342,235,85,c.panel,c.border,9);rect(253,359,25,25,'#365F56',c.border,4);txt(292,380,'#365F56',22);txt(253,409,'单击复制 · Shift 切换 RGB',14,c.muted);
  txt(100,540,'采样冻结底图，不包含标注、遮罩与工具控件。',19,c.muted);
  toolbar(110,634,['rect','arrow','pen','text','picker'],'picker');
  note(900,220,'进入时默认 HEX',['按一次 Shift → RGB','松开 Shift 后保持 RGB','再按一次 → HEX','长按不重复触发切换']);
  rect(900,435,320,92,c.panel,c.border,9);rect(919,455,25,25,'#365F56',c.border,4);txt(957,477,'RGB(54, 95, 86)',20);txt(920,509,'单击复制 · Shift 切换 HEX',15,c.muted);
  note(900,600,'连续取色',['复制成功后保持会话','透明间隙显示“无像素”','复制失败保留状态并提示']);
 }],
 ['s07-long-capture','S07  长截图 · 手动滚动采集','F11 · 进入时丢弃旧标注、草稿及撤销/重做；单屏纵向滚动',()=>{
  rect(50,140,860,590,c.side,c.border,12);txt(75,178,'实时页面 · 示意内容',16,c.muted);selection(90,220,755,420,false);txt(590,207,'采集中 · 选区锁定',15,c.muted);txt(124,270,'可滚动正文',26,c.text,600);content(124,310,650,7);
  rect(935,140,300,590,c.panel,c.border,12);txt(960,180,'长图预览',23,c.text,600);rect(969,204,231,380,c.paper,c.border,6);content(990,229,185,8);rect(981,440,207,121,'none',c.accent,3,2);txt(960,621,'755 × 4200 px',18,c.accent,600);button(960,658,111,'完成',true);button(1083,658,111,'取消');
  txt(90,686,'在正文滚动鼠标，自动拼接新增内容。',18,c.accent);txt(57,770,'框选时避开固定页头、底栏和悬浮内容；拼接不可靠时暂停并保留已成功部分。',18,c.muted);txt(57,801,'右侧预览内滚轮仅浏览预览；取消结束会话，下次仍默认普通截图。',18,c.muted);
 }],
 ['s08-long-edit','S08  长图完成后继续标注','F11 · 实时捕获已停止；主画布调参数，右侧缩略图定位',()=>{
  rect(45,140,1188,640,c.panel,c.border,12);txt(70,177,'长图编辑',23,c.text,600);txt(915,177,'755 × 4200 px',17,c.muted);line(65,196,1213,196);
  rect(80,218,855,426,c.side,'none',5);rect(225,218,565,426,c.paper,c.border,0);content(260,256,490,7);numberMark(280,410,1,20);rect(312,382,370,55,'none',c.red,0,3);txt(315,484,'继续添加标注',24,c.red);
  rect(970,218,225,426,c.paper,c.border,5);content(990,244,180,9);rect(981,318,202,95,'none',c.accent,3,2);txt(980,680,'拖动视口框定位',16,c.muted);toolbar(170,672,allTools.filter(x=>x!=='long'),'number');
  txt(65,809,'右侧可点击 / 拖动 / 滚轮浏览；输出完整长图。新一轮长截图需重新开始截图。',18,c.muted);
 }],
 ['s09-toolbar','S09  最终工具条与显示策略','F07 / F12 · 移除选择与撤销/重做按钮；保留绘制箭头及 Ctrl+Z / Ctrl+Y',()=>{
  rect(45,140,1190,610,c.panel,c.border,14);txt(80,190,'默认：新增工具关闭',23,c.text,600);toolbar(80,219,baseTools,'rect');txt(80,310,'矩形　箭头　画笔　文字　│　贴图　保存　复制　│　取消',16,c.muted);
  line(80,348,1197,348);txt(80,393,'全部开启：按固定顺序出现',23,c.text,600);toolbar(80,420,allTools,'mosaic');txt(80,512,'矩形　箭头　画笔　文字　马赛克　编号　│　长截图　取色　│　贴图　保存　复制　│　取消',16,c.muted);
  line(80,550,1197,550);txt(80,595,'小工作区：绘图工具收纳至“更多”，输出操作保持可达',21,c.text,600);toolbar(80,622,['more'],'');
  icon('copy',790,631,c.text);txt(835,652,'复制仅图标，不使用深色按下底色',17,c.muted);txt(835,684,'悬停、键盘焦点和失败反馈仍保留',17,c.muted);
  txt(65,795,'参数条按工具出现；关闭工具即收拢间距。下方优先，上方次之，最后安全收纳到屏幕内。',18,c.muted);
 }]
];

for(const theme of ['light','dark'])for(const [name,title,sub,fn] of scenes){fs.writeFileSync(path.join(out,`${name}-${theme}.svg`),scene(title,sub,theme,fn),'utf8');}
if(!process.argv.includes('--svg-only')){
 const browsers=[process.env.KIKI_SKETCH_BROWSER,'C:/Program Files/Google/Chrome/Application/chrome.exe','C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'].filter(Boolean);
 const browser=browsers.find(p=>fs.existsSync(p));
 if(!browser)throw new Error('需要已安装 Chrome/Edge，或设置 KIKI_SKETCH_BROWSER；SVG 已生成。');
 const profile=path.join(out,'.render-profile');
 for(const theme of ['light','dark'])for(const [name] of scenes){
  const svg=path.join(out,`${name}-${theme}.svg`), png=path.join(out,`${name}-${theme}.png`);
  const args=['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--hide-scrollbars',`--user-data-dir=${profile}`,'--force-device-scale-factor=1','--window-size=1280,900',`--screenshot=${png}`,pathToFileURL(svg).href];
  const r=spawnSync(browser,args,{windowsHide:true,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});
  if(r.error||r.status!==0||!fs.existsSync(png))throw new Error(`渲染 ${name}-${theme} 失败：${r.error||r.stderr}`);
  const b=fs.readFileSync(png);if(b.readUInt32BE(16)!==W||b.readUInt32BE(20)!==H)throw new Error('PNG 尺寸不符合预期');
  process.stdout.write(`${name}-${theme}.png\n`);
 }
 // Only delete our verified renderer profile, never a user browser profile.
 const resolved=path.resolve(profile);if(path.dirname(resolved)!==path.resolve(out)||path.basename(resolved)!=='.render-profile')throw new Error('渲染缓存路径校验失败');
 fs.rmSync(resolved,{recursive:true,force:true,maxRetries:5,retryDelay:200});
}
