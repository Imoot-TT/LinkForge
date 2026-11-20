import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import QRCode from "qrcode";
import "./App.css";
import "./Enhancements.css";
import "./AuditFixes.css";
import "./History.css";
import "./SettingsExtras.css";
import "./HistoryLoad.css";
import "./ThreeXui.css";
import "./Credential.css";
import "./Diagnostics.css";
import "./Qr.css";
import "./Theme.css";

type Page = "speed" | "nodes" | "settings";
type SpeedResult = { address:string; sent:number; received:number; packetLoss:number; averageLatency:number; downloadSpeed:number; colo:string };
type NodeChange = {protocol:string;name:string;originalAddress:string;preferredAddress:string;hostSniPreserved:boolean};
type LocalResult = { links:string[]; rawBase64:string; clashYaml:string; surgeProfile:string; inputNodes:number; preferredEndpoints:number; outputNodes:number; warnings:string[];changes:NodeChange[] };
type LocalFormat = "links"|"base64"|"mihomo"|"surge";
type NodePreview = { protocol:string; name:string; address:string; port:string; tls:string; transport:string; host:string; sni:string; valid:boolean; message:string };
type SortKey = "address"|"averageLatency"|"packetLoss"|"downloadSpeed";
type HistorySummary = { runId:string; createdAt:number; target:string; ipMode:string; testedCount:number; resultCount:number; bestLatency:number; bestSpeed:number; status:"completed"|"failed"|"cancelled"|"interrupted"|"running" };
type SubscriptionHistoryItem = {runId:string;createdAt:number;mode:"local"|"online";inputNodes:number;preferredEndpoints:number;outputNodes:number;formats:string[];endpointHost:string;status:"completed"|"failed";error:string};
type SpeedParams = {concurrency:number;attempts:number;downloadCount:number;downloadSeconds:number;port:number;latencyMin:number;latencyMax:number;packetLossMax:number;speedMin:number;disableDownload:boolean;httping:boolean};
type SpeedProfile = {id:string;name:string;testUrl:string;ipMode:"ipv4"|"ipv6";targets:string;params:SpeedParams;autoSelectCount?:number};
type SpeedRequest = SpeedParams&{targets:string;testUrl:string|null};
type HistoryRunResponse = {request:SpeedRequest;results:SpeedResult[]};
type PanelClient = {email:string;enabled:boolean;subId:string;inboundIds:number[]};
type PanelLinkCandidate = NodePreview&{link:string};
type ConfirmRequest = {title:string;message:string;confirmLabel:string;danger?:boolean;action:()=>void|Promise<void>};

const ipv4 = `173.245.48.0/20
103.21.244.0/22
103.22.200.0/22
103.31.4.0/22
141.101.64.0/18
108.162.192.0/18
190.93.240.0/20
188.114.96.0/20
197.234.240.0/22
198.41.128.0/17
162.158.0.0/15
104.16.0.0/12
172.64.0.0/17
172.64.128.0/18
172.64.192.0/19
172.64.224.0/22
172.64.229.0/24
172.64.230.0/23
172.64.232.0/21
172.64.240.0/21
172.64.248.0/21
172.65.0.0/16
172.66.0.0/16
172.67.0.0/16
131.0.72.0/22`;
const ipv6 = `2400:cb00::/32
2606:4700::/32
2803:f800::/32
2405:b500::/32
2405:8100::/32
2a06:98c0::/29
2c0f:f248::/32`;
const defaults = { baseUrl:"https://goodip.supconit.cc.cd", generatePath:"/api/generate", apiKey:"", enableOnline:false, skipPublishWarning:false, panelUrl:"", panelToken:"" };
const fail = (e:unknown) => typeof e === "string" ? e : e instanceof Error ? e.message : "操作失败";
const subscriptionCardSpecs=[
  {key:"auto",label:"自动识别",hint:"适合支持自动识别的客户端"},
  {key:"raw",label:"V2rayN",hint:"适用于 V2rayN / v2rayNG"},
  {key:"clash",label:"Clash / Mihomo",hint:"适用于 Clash、Mihomo、Clash Verge"},
  {key:"raw",label:"Shadowrocket",hint:"适用于 iPhone / iPad 小火箭"},
  {key:"surge",label:"Surge",hint:"适用于 Surge Profile 导入"},
] as const;

export default function App(){
  const [page,setPage]=useState<Page>("speed");
  const [settings,setSettings]=useState(()=>{try{return {...defaults,...JSON.parse(localStorage.getItem("iptools.settings")||"{}"),apiKey:""}}catch{return defaults}});
  const [ipMode,setIpMode]=useState<"ipv4"|"ipv6">("ipv4");
  const [testUrl,setTestUrl]=useState("");
  const [params,setParams]=useState<SpeedParams>({concurrency:200,attempts:4,downloadCount:10,downloadSeconds:10,port:443,latencyMin:0,latencyMax:9999,packetLossMax:1,speedMin:0,disableDownload:false,httping:false});
  const [autoSelectCount,setAutoSelectCount]=useState(10);
  const [profiles,setProfiles]=useState<SpeedProfile[]>(()=>{try{return JSON.parse(localStorage.getItem("iptools.speedProfiles")||"[]")}catch{return[]}});
  const [activeProfile,setActiveProfile]=useState("");
  const [advanced,setAdvanced]=useState(false);
  const [targets,setTargets]=useState(ipv4);
  const [running,setRunning]=useState(false);
  const [startedAt,setStartedAt]=useState<number|null>(null);
  const [elapsed,setElapsed]=useState(0);
  const [stage,setStage]=useState("就绪");
  const [logs,setLogs]=useState<string[]>([]);
  const [results,setResults]=useState<SpeedResult[]>([]);
  const [resultSearch,setResultSearch]=useState("");
  const [resultPage,setResultPage]=useState(0);
  const [sortKey,setSortKey]=useState<SortKey>("downloadSpeed");
  const [sortDesc,setSortDesc]=useState(true);
  const [selected,setSelected]=useState<Set<string>>(new Set());
  const [notice,setNotice]=useState("");
  const [nodeLinks,setNodeLinks]=useState("");
  const [preferredIps,setPreferredIps]=useState("");
  const [namePrefix,setNamePrefix]=useState("");
  const [keepOriginalHost,setKeepOriginalHost]=useState(true);
  const [localResult,setLocalResult]=useState<LocalResult|null>(null);
  const [localFormat,setLocalFormat]=useState<LocalFormat>("mihomo");
  const [nodePreview,setNodePreview]=useState<NodePreview[]>([]);
  const [onlineResult,setOnlineResult]=useState<Record<string,any>|null>(null);
  const [publishing,setPublishing]=useState(false);
  const [publishConsentOpen,setPublishConsentOpen]=useState(false);
  const [rememberPublishConsent,setRememberPublishConsent]=useState(false);
  const [history,setHistory]=useState<HistorySummary[]>([]);
  const [subscriptionHistory,setSubscriptionHistory]=useState<SubscriptionHistoryItem[]>([]);
  const [historySearch,setHistorySearch]=useState("");
  const [historyDays,setHistoryDays]=useState<0|7|30>(0);
  const [historyStatus,setHistoryStatus]=useState("all");
  const [testingService,setTestingService]=useState(false);
  const [loadingPanel,setLoadingPanel]=useState(false);
  const panelFilter="";
  const [panelTokenStored,setPanelTokenStored]=useState(false);
  const [panelSessionActive,setPanelSessionActive]=useState(false);
  const [panelAuthMode,setPanelAuthMode]=useState<"login"|"token">("login");
  const [panelLogin,setPanelLogin]=useState({username:"",password:"",twoFactorCode:""});
  const [panelLoginSaved,setPanelLoginSaved]=useState(false);
  const [rememberPanelLogin,setRememberPanelLogin]=useState(true);
  const [loggingIn,setLoggingIn]=useState(false);
  const [showPanelPassword,setShowPanelPassword]=useState(false);
  const [showApiKey,setShowApiKey]=useState(false);
  const panelLoginFieldsRef=useRef<HTMLDivElement>(null);
  const [diagnosticsPath,setDiagnosticsPath]=useState("");
  const [panelClients,setPanelClients]=useState<PanelClient[]>([]);
  const [selectedPanelClient,setSelectedPanelClient]=useState("");
  const [panelCandidates,setPanelCandidates]=useState<PanelLinkCandidate[]>([]);
  const [selectedPanelLinks,setSelectedPanelLinks]=useState<Set<number>>(new Set());
  const autoLoginUrl=useRef("");
  const [qr,setQr]=useState<{label:string;data:string}|null>(null);
  const [confirmRequest,setConfirmRequest]=useState<ConfirmRequest|null>(null);

  useEffect(()=>{const p=Promise.all([
    listen<string>("speed-log",e=>setLogs(v=>[...v.slice(-299),e.payload])),
    listen<string>("speed-stage",e=>{setStage(e.payload);setLogs(v=>[...v.slice(-299),e.payload])})
  ]);return()=>{p.then(v=>v.forEach(f=>f()))}},[]);
  useEffect(()=>{refreshHistory();refreshSubscriptionHistory();invoke<Array<{runId:string;startedAt:number;target:string;ipMode:string}>>("recover_unfinished_tasks").then(tasks=>{if(tasks.length){const latest=tasks[0];setNotice(`检测到 ${tasks.length} 个上次异常中断的测速任务；最近一次为 ${latest.ipMode} · ${latest.target}。参数和已完成历史仍可从下方载入。`)}}).catch(()=>{/* 不阻断主流程 */})},[]);
  useEffect(()=>{if(!running||startedAt===null)return;const tick=()=>setElapsed(Math.floor((Date.now()-startedAt)/1000));tick();const timer=setInterval(tick,1000);return()=>clearInterval(timer)},[running,startedAt]);
  useEffect(()=>{if(!notice)return;const timer=setTimeout(()=>setNotice(""),6500);return()=>clearTimeout(timer)},[notice]);
  useEffect(()=>{if(running||stage==="就绪")return;const timer=setTimeout(()=>setStage("就绪"),4500);return()=>clearTimeout(timer)},[running,stage]);
  useEffect(()=>{if(!settings.panelUrl){setPanelTokenStored(false);setPanelLoginSaved(false);setPanelSessionActive(false);autoLoginUrl.current="";return}const panelUrl=settings.panelUrl;const timer=setTimeout(()=>{invoke<boolean>("panel_token_status",{panelUrl}).then(setPanelTokenStored).catch(()=>setPanelTokenStored(false));invoke<boolean>("panel_session_status",{panelUrl}).then(setPanelSessionActive).catch(()=>setPanelSessionActive(false));invoke<{saved:boolean;username?:string}>("panel_login_status",{panelUrl}).then(status=>{setPanelLoginSaved(status.saved);if(status.username)setPanelLogin(v=>({...v,username:status.username||""}));if(status.saved&&autoLoginUrl.current!==panelUrl){autoLoginUrl.current=panelUrl;setLoggingIn(true);invoke("login_saved_three_xui",{panelUrl,twoFactorCode:""}).then(()=>{setPanelSessionActive(true);setNotice("已使用 Windows 凭据管理器中的账号自动登录 3X-UI。")}).catch(e=>{const message=fail(e);if(message.includes("两步验证"))setNotice("已找到保存的 3X-UI 账号；请输入本次 2FA 验证码后登录。");else setNotice(message)}).finally(()=>setLoggingIn(false))}}).catch(()=>setPanelLoginSaved(false))},350);return()=>clearTimeout(timer)},[settings.panelUrl]);

  const ranked=useMemo(()=>[...results].filter(v=>v.address.toLowerCase().includes(resultSearch.toLowerCase())||v.colo.toLowerCase().includes(resultSearch.toLowerCase())).sort((a,b)=>{
    const av=a[sortKey],bv=b[sortKey];const n=typeof av==="string"?av.localeCompare(String(bv)):Number(av)-Number(bv);return sortDesc?-n:n
  }),[results,resultSearch,sortKey,sortDesc]);
  const picked=useMemo(()=>ranked.filter(v=>selected.has(v.address)),[ranked,selected]);
  const resultPageCount=Math.max(1,Math.ceil(ranked.length/100));
  const visibleResults=useMemo(()=>ranked.slice(resultPage*100,resultPage*100+100),[ranked,resultPage]);
  const filteredHistory=useMemo(()=>{const needle=historySearch.trim().toLowerCase();const cutoff=historyDays?Date.now()/1000-historyDays*86400:0;return history.filter(item=>(!cutoff||item.createdAt>=cutoff)&&(historyStatus==="all"||item.status===historyStatus)&&(!needle||item.target.toLowerCase().includes(needle)||item.ipMode.toLowerCase().includes(needle)))},[history,historySearch,historyDays,historyStatus]);
  const consoleProgress=useMemo(()=>{const match=stage.match(/(\d+)\/(\d+)/);return match?Math.min(100,Math.round(Number(match[1])/Math.max(1,Number(match[2]))*100)):stage.includes("已完成")?100:running?4:0},[stage,running]);
  const runStep=stage.includes("延迟")?1:stage.includes("下载")?2:stage.includes("已完成")?3:0;
  const runCounter=stage.match(/(\d+)\/(\d+)/);
  const latestLog=logs.length?logs[logs.length-1]:"等待开始测速";
  useEffect(()=>{if(resultPage>=resultPageCount)setResultPage(resultPageCount-1)},[resultPage,resultPageCount]);

  async function executeSpeedRequest(request:SpeedRequest){
    setNotice("");setLogs([]);setRunning(true);setStartedAt(Date.now());setElapsed(0);setStage("准备测速");
    try{
      const r=await invoke<{results:SpeedResult[];persistenceWarning?:string|null}>("run_native_speed_test",{request});
      setResults(r.results);const top=[...r.results].sort((a,b)=>b.downloadSpeed-a.downloadSpeed||a.averageLatency-b.averageLatency).slice(0,Math.max(1,Math.min(50,autoSelectCount)));
      setSelected(new Set(top.map(v=>v.address)));setPreferredIps(top.map((v,i)=>`${v.address}#优选-${i+1}`).join("\n"));
      localStorage.setItem("iptools.lastRun",JSON.stringify({at:new Date().toISOString(),tested:r.results.length,selected:top.length,mode:request.targets.includes(":")&&!request.targets.includes(".")?"ipv6":"ipv4",url:request.testUrl?new URL(request.testUrl).hostname:"TCP only"}));
      const allZero=!request.disableDownload&&r.results.length>0&&r.results.every(v=>v.downloadSpeed<=0);
       setNotice(`完成：找到 ${r.results.length} 个可用入口，已选择前 ${top.length} 个。${allZero?"下载速度全为 0；CFST 中部分回源 IP 会出现这种结果，可将最低速度设为 0.01 过滤，或改用自建测速地址。":""}${r.persistenceWarning?` 历史保存失败：${r.persistenceWarning}`:""}`);refreshHistory();
    }catch(e){setStage("失败");setNotice(fail(e))}finally{setRunning(false)}
  }
  async function start(){await executeSpeedRequest({targets,testUrl:testUrl||null,...params})}
  async function cancel(){try{await invoke("cancel_speed_test");setStage("已取消")}catch(e){setNotice(fail(e))}}
  function toggle(ip:string){setSelected(old=>{const n=new Set(old);n.has(ip)?n.delete(ip):n.add(ip);return n})}
  function next(){setPreferredIps(picked.map((v,i)=>`${v.address}#优选-${i+1}`).join("\n"));setPage("nodes")}
  async function local(){try{const r=await invoke<LocalResult>("generate_local",{request:{nodeLinks,preferredIps,namePrefix,keepOriginalHost}});setLocalResult(r);setLocalFormat("mihomo");setOnlineResult(null);await recordSubscriptionHistory("local",r);setNotice(`已在本机生成 ${r.outputNodes} 个节点${r.warnings.length?`，另有 ${r.warnings.length} 条格式兼容提示` : ""}。`)}catch(e){const message=fail(e);await recordSubscriptionHistory("local",{inputNodes:nodeLinks.split(/\r?\n/).filter(Boolean).length,preferredEndpoints:preferredIps.split(/\r?\n/).filter(Boolean).length},"failed",message);setNotice(message)}}
  async function preview(){try{const r=await invoke<NodePreview[]>("preview_nodes",{input:nodeLinks});setNodePreview(r);setNotice(`已解析 ${r.length} 个节点，敏感凭据未返回界面。`)}catch(e){setNotice(fail(e))}}
  async function publishOnline(){setPublishConsentOpen(false);if(rememberPublishConsent){const next={...settings,skipPublishWarning:true};setSettings(next);const{apiKey:_,panelToken:__,...safe}=next;localStorage.setItem("iptools.settings",JSON.stringify(safe))}setPublishing(true);try{const r=await invoke<{data:Record<string,any>}>("publish_goodip",{request:{baseUrl:settings.baseUrl,generatePath:settings.generatePath,apiKey:settings.apiKey||null,nodeLinks,preferredIps,namePrefix,keepOriginalHost}});setOnlineResult(r.data);await recordSubscriptionHistory("online",r.data);setNotice("在线订阅生成成功。") }catch(e){const message=fail(e);await recordSubscriptionHistory("online",{counts:{inputNodes:nodeLinks.split(/\r?\n/).filter(Boolean).length,preferredEndpoints:preferredIps.split(/\r?\n/).filter(Boolean).length}},"failed",message);setNotice(message)}finally{setPublishing(false)}}
  async function online(){if(!settings.enableOnline){setNotice("第三方在线发布已在设置中关闭。");return}if(!settings.baseUrl.trim()||!settings.generatePath.trim()){setNotice("在线生成需要先填写发布服务和接口路径。");return}if(!settings.skipPublishWarning){setRememberPublishConsent(false);setPublishConsentOpen(true);return}await publishOnline()}
  async function copy(v:string){await navigator.clipboard.writeText(v);setNotice("已复制。")}
  function downloadText(name:string,value:string,type="text/plain;charset=utf-8"){const blob=new Blob([value],{type});const url=URL.createObjectURL(blob);const a=document.createElement("a");a.href=url;a.download=name;a.click();URL.revokeObjectURL(url);setNotice(`已导出 ${name}。`)}
  async function showQr(label:string,value:string){try{setQr({label,data:await QRCode.toDataURL(value,{width:240,margin:1,errorCorrectionLevel:"M"})})}catch(e){setNotice(fail(e))}}
  async function refreshHistory(){try{setHistory(await invoke<HistorySummary[]>("list_history"))}catch{/* 首次启动时不阻断主流程 */}}
  async function refreshSubscriptionHistory(){try{setSubscriptionHistory(await invoke<SubscriptionHistoryItem[]>("list_subscription_history"))}catch{/* 首次启动时不阻断主流程 */}}
  function clearHistory(){setConfirmRequest({title:"清空历史",message:"测速历史和订阅生成历史将被删除。当前结果、设置和凭据不会受到影响。",confirmLabel:"清空历史",danger:true,action:async()=>{try{await invoke("clear_history");setHistory([]);setSubscriptionHistory([]);setNotice("历史已清空。")}catch(e){setNotice(fail(e))}}})}
  async function recordSubscriptionHistory(mode:"local"|"online", result:any, status:"completed"|"failed"="completed", error=""){
    const counts=result?.counts||{};
    const formats=mode==="local"?["Mihomo","Surge","Base64","节点链接"]:Object.keys(result?.urls||{});
    let endpointHost="";
    if(mode==="online"){try{endpointHost=new URL(settings.baseUrl).hostname}catch{/* 不阻断结果展示 */}}
    await invoke("record_subscription_history",{request:{mode,inputNodes:result?.inputNodes??counts.inputNodes??0,preferredEndpoints:result?.preferredEndpoints??counts.preferredEndpoints??0,outputNodes:result?.outputNodes??counts.outputNodes??0,formats,endpointHost,status,error:error.slice(0,500)}}).catch(()=>{});
    refreshSubscriptionHistory();
  }
  async function loadHistory(runId:string,rerun=false){try{const r=await invoke<HistoryRunResponse>("get_history_run",{runId});const restoredParams:SpeedParams={concurrency:r.request.concurrency,attempts:r.request.attempts,downloadCount:r.request.downloadCount,downloadSeconds:r.request.downloadSeconds,port:r.request.port,latencyMin:r.request.latencyMin||0,latencyMax:r.request.latencyMax,packetLossMax:r.request.packetLossMax,speedMin:r.request.speedMin,disableDownload:r.request.disableDownload,httping:r.request.httping};setTargets(r.request.targets);setIpMode(r.request.targets.includes(":")&&!r.request.targets.includes(".")?"ipv6":"ipv4");setTestUrl(r.request.testUrl||"");setParams(restoredParams);setResults(r.results);const top=[...r.results].sort((a,b)=>b.downloadSpeed-a.downloadSpeed||a.averageLatency-b.averageLatency).slice(0,Math.max(1,Math.min(50,autoSelectCount)));setSelected(new Set(top.map(v=>v.address)));setPreferredIps(top.map((v,i)=>`${v.address}#优选-${i+1}`).join("\n"));if(rerun){await executeSpeedRequest({...r.request,latencyMin:r.request.latencyMin||0})}else setNotice("已载入历史结果和参数；可直接调整选择或重新测速。")}catch(e){setNotice(fail(e))}}
  function save(){if(settings.enableOnline&&(!settings.baseUrl.trim()||!settings.generatePath.trim())){setNotice("启用在线订阅时，发布服务和接口路径都是必填项。");return}const{apiKey:_,panelToken:__,...safe}=settings;localStorage.setItem("iptools.settings",JSON.stringify(safe));setNotice("设置已保存；API Key 和 3X-UI Token 不会写入应用配置。")}
  async function fetchPanel(){setLoadingPanel(true);try{const r=await invoke<{links:string[];count:number;source:string}>("fetch_three_xui_links",{request:{panelUrl:settings.panelUrl,apiToken:settings.panelToken,filter:panelFilter}});setNodeLinks(r.links.join("\n"));setNotice(`已通过 ${r.source} 从 3X-UI 获取 ${r.count} 个分享链接${panelFilter.trim()?`（筛选：${panelFilter.trim()}）`:""}。`);setNodePreview(await invoke<NodePreview[]>("preview_nodes",{input:r.links.join("\n")}))}catch(e){setNotice(fail(e))}finally{setLoadingPanel(false)}}
  async function storePanelToken(){try{await invoke("save_panel_token",{panelUrl:settings.panelUrl,token:settings.panelToken});setPanelTokenStored(true);setSettings({...settings,panelToken:""});setNotice("3X-UI Token 已保存到 Windows 凭据管理器，应用配置和数据库中没有明文副本。")}catch(e){setNotice(fail(e))}}
  function removePanelToken(){setConfirmRequest({title:"删除 3X-UI Token",message:"将从 Windows 凭据管理器删除这个面板保存的 Token。当前应用会话不受影响。",confirmLabel:"删除 Token",danger:true,action:async()=>{try{await invoke("delete_panel_token",{panelUrl:settings.panelUrl});setPanelTokenStored(false);setNotice("已删除保存的 3X-UI Token。")}catch(e){setNotice(fail(e))}}})}
  async function loginAndOptionallySave(){
    const fields=panelLoginFieldsRef.current?.querySelectorAll<HTMLInputElement>("input") ?? [];
    const login={
      username:fields[0]?.value ?? panelLogin.username,
      password:fields[1]?.value ?? panelLogin.password,
      twoFactorCode:fields[2]?.value ?? panelLogin.twoFactorCode,
    };
    if(!settings.panelUrl.trim()){setNotice("请先填写 3X-UI 面板 URL。");return}
    if(!login.username.trim()||!login.password){setNotice("请填写 3X-UI 用户名和密码。");return}
    setPanelLogin(login);
    setLoggingIn(true);
    try{
      await invoke("login_three_xui",{request:{panelUrl:settings.panelUrl,...login}});
      if(rememberPanelLogin){await invoke("save_panel_login",{panelUrl:settings.panelUrl,username:login.username,password:login.password});setPanelLoginSaved(true)}
      setPanelSessionActive(true);
      setPanelLogin({...login,password:"",twoFactorCode:""});
      setNotice(rememberPanelLogin?"登录成功；账号密码已保存到 Windows 凭据管理器，下次启动会自动登录。":"登录成功；会话 Cookie 仅保存在本次应用运行的内存中。")
    }catch(e){setNotice(fail(e))}finally{setLoggingIn(false)}
  }
  async function loginWithSaved(){
    if(!settings.panelUrl.trim()){setNotice("请先填写 3X-UI 面板 URL。");return}
    setLoggingIn(true);
    try{await invoke("login_saved_three_xui",{panelUrl:settings.panelUrl,twoFactorCode:panelLogin.twoFactorCode});setPanelSessionActive(true);setPanelLogin({...panelLogin,twoFactorCode:""});setNotice("已使用保存的账号登录 3X-UI。") }catch(e){setNotice(fail(e))}finally{setLoggingIn(false)}
  }
  function removePanelLogin(){setConfirmRequest({title:"删除保存的账号",message:"将从 Windows 凭据管理器删除这个面板保存的用户名和密码。当前内存会话不受影响。",confirmLabel:"删除账号",danger:true,action:async()=>{try{await invoke("delete_panel_login",{panelUrl:settings.panelUrl});setPanelLoginSaved(false);autoLoginUrl.current="";setNotice("已删除保存的 3X-UI 登录凭据；当前内存会话不受影响。") }catch(e){setNotice(fail(e))}}})}
  async function forgetPanelSession(){try{await invoke("forget_panel_session",{panelUrl:settings.panelUrl});setPanelSessionActive(false);setNotice("已从本机内存中清除 3X-UI 登录会话；下次读取前需要重新登录。")}catch(e){setNotice(fail(e))}}
  async function loadPanelClients(){setLoadingPanel(true);try{const clients=await invoke<PanelClient[]>("list_three_xui_clients",{request:{panelUrl:settings.panelUrl,apiToken:settings.panelToken,email:null}});setPanelClients(clients);const mt=clients.find(v=>v.email.toLowerCase()==="mt");setSelectedPanelClient(mt?.email||clients[0]?.email||"");setPanelCandidates([]);setNotice(`已读取 ${clients.length} 个 3X-UI 客户端${mt?"，已优先选中 mt":""}。`) }catch(e){setNotice(fail(e))}finally{setLoadingPanel(false)}}
  async function loadPanelClientLinks(){if(!selectedPanelClient)return;setLoadingPanel(true);try{const r=await invoke<{email:string;links:string[];count:number}>("fetch_three_xui_client_links",{request:{panelUrl:settings.panelUrl,apiToken:settings.panelToken,email:selectedPanelClient}});const previews=await invoke<NodePreview[]>("preview_nodes",{input:r.links.join("\n")});const candidates=r.links.map((link,index)=>({...previews[index],link}));setPanelCandidates(candidates);setSelectedPanelLinks(new Set(candidates.map((_,index)=>index)));setNotice(`客户端 ${r.email} 关联 ${r.count} 个节点，请确认后使用。`) }catch(e){setNotice(fail(e))}finally{setLoadingPanel(false)}}
  function useSelectedPanelLinks(){const links=panelCandidates.filter((_,index)=>selectedPanelLinks.has(index)).map(v=>v.link);setNodeLinks(links.join("\n"));setNodePreview(panelCandidates.filter((_,index)=>selectedPanelLinks.has(index)));setNotice(`已使用客户端 ${selectedPanelClient} 的 ${links.length} 个节点。`)}
  async function testService(){if(!settings.baseUrl.trim()||!settings.generatePath.trim()){setNotice("请先填写发布服务和接口路径。");return}setTestingService(true);try{const r=await invoke<{status:number;note:string}>("test_publish_connection",{request:{baseUrl:settings.baseUrl,generatePath:settings.generatePath,apiKey:settings.apiKey||null}});setNotice(`连接结果：HTTP ${r.status}。${r.note}`)}catch(e){setNotice(fail(e))}finally{setTestingService(false)}}
  async function exportDiagnostics(){try{const result=await invoke<{path:string}>("export_diagnostics_file");setDiagnosticsPath(result.path);setNotice(`脱敏诊断包已保存到：${result.path}`)}catch(e){setNotice(fail(e))}}
  function preset(kind:"fast"|"cfst"|"strict") { const values={fast:{concurrency:300,attempts:2,downloadCount:5,downloadSeconds:5,latencyMax:350,packetLossMax:.5},cfst:{concurrency:200,attempts:4,downloadCount:10,downloadSeconds:10,latencyMax:9999,packetLossMax:1},strict:{concurrency:100,attempts:6,downloadCount:20,downloadSeconds:12,latencyMax:180,packetLossMax:.05}}[kind];setParams({...params,...values}) }
  function changeSort(key:SortKey){if(sortKey===key)setSortDesc(!sortDesc);else{setSortKey(key);setSortDesc(key!=="address")}}
  function exportCsv(){if(!results.length)return;const q=(v:unknown)=>`"${String(v).replaceAll('"','""')}"`;const rows=[["IP","已发送","已接收","丢包率","平均延迟(ms)","下载速度(MB/s)","地区"],...results.map(v=>[v.address,v.sent,v.received,v.packetLoss,v.averageLatency,v.downloadSpeed,v.colo])];const blob=new Blob(["\ufeff"+rows.map(row=>row.map(q).join(",")).join("\r\n")],{type:"text/csv;charset=utf-8"});const url=URL.createObjectURL(blob);const a=document.createElement("a");a.href=url;a.download=`linkforge-${new Date().toISOString().slice(0,10)}.csv`;a.click();URL.revokeObjectURL(url)}
  function persistProfiles(next:SpeedProfile[]){setProfiles(next);localStorage.setItem("iptools.speedProfiles",JSON.stringify(next))}
  function saveProfile(){let suggested="IP 优选";try{if(testUrl.trim())suggested=new URL(testUrl).hostname}catch{/* 输入错误由测速时统一提示 */}const name=prompt("方案名称",suggested)?.trim();if(!name)return;const existing=profiles.find(v=>v.name===name);const profile:SpeedProfile={id:existing?.id||crypto.randomUUID(),name,testUrl,ipMode,targets,params:{...params},autoSelectCount};const next=existing?profiles.map(v=>v.id===existing.id?profile:v):[...profiles,profile];persistProfiles(next);setActiveProfile(profile.id);setNotice(`已保存测速方案“${name}”。`)}
  function applyProfile(id:string){setActiveProfile(id);const profile=profiles.find(v=>v.id===id);if(!profile)return;setTestUrl(profile.testUrl);setIpMode(profile.ipMode);setTargets(profile.targets);setParams({...profile.params});setAutoSelectCount(profile.autoSelectCount??10);setNotice(`已载入测速方案“${profile.name}”。`)}
  function deleteProfile(){if(!activeProfile)return;const profile=profiles.find(v=>v.id===activeProfile);if(!profile)return;setConfirmRequest({title:"删除测速方案",message:`确定删除“${profile.name}”？此操作不会影响测速历史。`,confirmLabel:"删除方案",danger:true,action:()=>{persistProfiles(profiles.filter(v=>v.id!==activeProfile));setActiveProfile("");setNotice("测速方案已删除。");}})}

  return <div className="app">
     {confirmRequest&&<div className="confirm-backdrop" role="dialog" aria-modal="true" aria-labelledby="confirm-title" onClick={()=>setConfirmRequest(null)}><div className="confirm-dialog" onClick={e=>e.stopPropagation()}><div className="confirm-dialog-head"><span className={confirmRequest.danger?"confirm-mark danger":"confirm-mark"}>!</span><div><h2 id="confirm-title">{confirmRequest.title}</h2><p>{confirmRequest.title==="清空历史"?"删除测速与订阅历史？当前结果、设置和凭据不受影响。":confirmRequest.message}</p></div></div><div className="confirm-actions"><button className="ghost" onClick={()=>setConfirmRequest(null)}>取消</button><button className={confirmRequest.danger?"confirm-danger":"run"} onClick={()=>{const request=confirmRequest;setConfirmRequest(null);void request.action()}}>{confirmRequest.confirmLabel}</button></div></div></div>}
     {qr&&<div className="qr-backdrop" role="dialog" aria-modal="true" aria-label={`${qr.label} 二维码`} onClick={()=>setQr(null)}><div className="qr-dialog" onClick={e=>e.stopPropagation()}><div><b>{qr.label}</b><button aria-label="关闭二维码" onClick={()=>setQr(null)}>×</button></div><img src={qr.data} alt={`${qr.label} 二维码`}/><p>请使用对应客户端扫描；订阅二维码包含访问地址，请勿公开分享。</p></div></div>}
    {publishConsentOpen&&<div className="consent-backdrop" role="dialog" aria-modal="true" aria-labelledby="publish-consent-title" onClick={()=>setPublishConsentOpen(false)}><div className="consent-dialog" onClick={e=>e.stopPropagation()}><h2 id="publish-consent-title">发送节点到第三方服务？</h2><p>完整节点链接包含 UUID、密码或订阅凭据，将发送到：</p><code>{settings.baseUrl}{settings.generatePath}</code><p>LinkForge 无法控制该服务如何保存或处理节点。你也可以取消并使用“本地生成”。</p><label><input type="checkbox" checked={rememberPublishConsent} onChange={e=>setRememberPublishConsent(e.target.checked)}/><span>记住选择，以后不再提示（可在设置中恢复）</span></label><div><button className="ghost" onClick={()=>setPublishConsentOpen(false)}>取消</button><button className="run" onClick={publishOnline}>确认并发送</button></div></div></div>}
     <header className="topbar"><div className="logo"><div><b>LinkForge</b><small>边缘优选 · 节点与订阅工作台</small></div></div>
      <nav><button className={page==="speed"?"on":""} onClick={()=>setPage("speed")}>1&nbsp; IP 优选</button><button className={page==="nodes"?"on":""} onClick={()=>setPage("nodes")}>2&nbsp; 生成订阅</button><button className={page==="settings"?"on":""} onClick={()=>setPage("settings")}>设置</button></nav>
    </header>
    <main>
      {notice&&<div className="toast">{notice}<button onClick={()=>setNotice("")}>×</button></div>}

      {page==="speed"&&<>
         <section className="hero"><div><span>步骤 1</span><h1>IP 优选</h1><p>本机 → 边缘节点</p></div><div className="step"><b>1</b><i/><em>2</em></div></section>
         <section className="panel primary-panel">
          <div className="quick-row"><div className="seg"><button className={ipMode==="ipv4"?"active":""} onClick={()=>{setIpMode("ipv4");setTargets(ipv4)}}>IPv4</button><button className={ipMode==="ipv6"?"active":""} onClick={()=>{setIpMode("ipv6");setTargets(ipv6)}}>IPv6</button></div>
             <div className="presets"><button onClick={()=>preset("fast")}>快速</button><button onClick={()=>preset("cfst")}>CFST 参数</button><button onClick={()=>preset("strict")}>严格</button></div>
            <label title="延迟筛选后参与下载测速的候选数量"><span>下载测速</span><input type="number" min="1" max="100" value={params.downloadCount} onChange={e=>setParams({...params,downloadCount:+e.target.value})}/><small>个</small></label>
            <label title="测速完成后自动勾选排名最前的结果"><span>自动选择</span><input type="number" min="1" max="50" value={autoSelectCount} onChange={e=>setAutoSelectCount(Math.max(1,Math.min(50,+e.target.value||1)))}/><small>个</small></label>
            <label><span>延迟上限</span><input type="number" value={params.latencyMax} onChange={e=>setParams({...params,latencyMax:+e.target.value})}/><small>ms</small></label>
            <button className="link" onClick={()=>setAdvanced(!advanced)}>{advanced?"收起":"高级设置"}</button>
            <button className="run" disabled={running} onClick={start}>{running?"正在测速…":"开始优选"}</button>{running&&<button className="stop" onClick={cancel}>停止</button>}
          </div>
           {advanced&&<div className="advanced">
             <label className="advanced-url"><span>自定义下载测速地址（可选）</span><input value={testUrl} onChange={e=>setTestUrl(e.target.value)} placeholder="留空使用 CFST 默认地址"/><small>留空先使用 CFST 默认地址；若某个入口被拒绝，应用会自动尝试官方下载接口。</small></label>
            <div className="numbers">{([['concurrency','并发'],['attempts','次数'],['downloadSeconds','下载秒数'],['port','端口'],['latencyMin','延迟下限'],['packetLossMax','丢包上限'],['speedMin','速度下限']] as const).map(([k,n])=><label key={k}><span>{n}</span><input type="number" value={params[k]} onChange={e=>setParams({...params,[k]:+e.target.value})}/></label>)}</div>
            <label className="check"><input type="checkbox" checked={params.httping} onChange={e=>setParams({...params,httping:e.target.checked})}/> 使用 HTTPing 验证域名</label>
            <details><summary>Cloudflare 候选网段（通常无需修改）</summary><textarea value={targets} onChange={e=>setTargets(e.target.value)}/></details>
            <div className="profile-bar advanced-profile"><select aria-label="测速方案" value={activeProfile} onChange={e=>applyProfile(e.target.value)}><option value="">选择已保存方案</option>{profiles.map(profile=><option key={profile.id} value={profile.id}>{profile.name}</option>)}</select><button className="ghost" onClick={saveProfile}>保存当前方案</button>{activeProfile&&<button className="profile-delete" onClick={deleteProfile}>删除</button>}</div>
          </div>}
        </section>
         {(running||logs.length>0)&&<section className="panel speed-progress-panel" aria-live="polite"><div className="speed-progress-head"><div><h2>测速进度</h2><span>{stage}{running?` · ${elapsed}s`:""}</span></div><button className="ghost" onClick={()=>setLogs([])}>清除记录</button></div><div className="speed-steps"><span className={runStep>=1?"active":""}><i>1</i>延迟</span><b/><span className={runStep>=2?"active":""}><i>2</i>下载</span><b/><span className={runStep>=3?"active":""}><i>3</i>完成</span></div><div className="speed-progress-track"><i style={{width:`${consoleProgress}%`}}/></div><div className="speed-progress-meta"><span>当前阶段 <strong>{stage}</strong></span>{runCounter&&<span>已处理 {runCounter[1]} / {runCounter[2]}</span>}<em title={latestLog}>{latestLog}</em></div></section>}
        <section className="panel result-panel"><div className="panel-title"><div><h2>优选结果</h2><p>{results.length?`可用 ${results.length} · 已选 ${selected.size}`:"完成测速后显示最快入口"}</p></div><div className="result-actions"><input aria-label="搜索结果" value={resultSearch} onChange={e=>{setResultSearch(e.target.value);setResultPage(0)}} placeholder="搜索 IP / 地区"/><button className="ghost" disabled={!selected.size} onClick={()=>copy(picked.map(v=>v.address).join("\n"))}>复制已选</button><button className="ghost" disabled={!results.length} onClick={exportCsv}>导出 CSV</button><button className="next" disabled={!selected.size} onClick={next}>生成订阅 →</button></div></div>
          <div className="table"><div className="tr head"><span></span><button onClick={()=>changeSort("address")}>入口 IP</button><button onClick={()=>changeSort("averageLatency")}>延迟</button><button onClick={()=>changeSort("packetLoss")}>丢包</button><button onClick={()=>changeSort("downloadSpeed")}>速度</button><span>地区</span></div>
            {visibleResults.length?visibleResults.map(v=><div className="tr" key={v.address}><span><input type="checkbox" checked={selected.has(v.address)} onChange={()=>toggle(v.address)}/></span><b>{v.address}</b><span>{v.averageLatency.toFixed(0)} ms</span><span>{(v.packetLoss*100).toFixed(0)}%</span><strong>{v.downloadSpeed.toFixed(2)} MB/s</strong><em>{v.colo}</em></div>):<div className="empty">尚未测速</div>}
          </div>{ranked.length>100&&<div className="result-pager"><span>共 {ranked.length} 条 · 第 {resultPage+1}/{resultPageCount} 页</span><button disabled={resultPage===0} onClick={()=>setResultPage(v=>Math.max(0,v-1))}>上一页</button><button disabled={resultPage>=resultPageCount-1} onClick={()=>setResultPage(v=>Math.min(resultPageCount-1,v+1))}>下一页</button></div>}
        </section>
        {history.length>0&&<section className="panel history-panel"><div className="panel-title"><div><h2>测速历史</h2><p>完整参数和结果保存在本机 SQLite 数据库</p></div><button className="history-clear" onClick={clearHistory}>清空历史</button><div className="history-filters"><input value={historySearch} onChange={e=>setHistorySearch(e.target.value)} placeholder="搜索目标 / IP 版本"/><select value={historyStatus} onChange={e=>setHistoryStatus(e.target.value)}><option value="all">全部状态</option><option value="completed">已完成</option><option value="failed">失败</option><option value="cancelled">已取消</option><option value="interrupted">异常中断</option></select><select value={historyDays} onChange={e=>setHistoryDays(Number(e.target.value) as 0|7|30)}><option value={0}>全部时间</option><option value={7}>最近 7 天</option><option value={30}>最近 30 天</option></select></div></div><div className="history-list">{filteredHistory.slice(0,20).map(item=>{const completed=item.status==="completed";const statusText={completed:"已完成",failed:"失败",cancelled:"已取消",interrupted:"异常中断",running:"进行中"}[item.status];return <div className="history-row" key={item.runId}><span>{new Date(item.createdAt*1000).toLocaleString()}</span><b>{item.target}</b><span>{item.ipMode}</span><span className={`history-status ${item.status}`}>{statusText}</span><span>{completed?`${item.resultCount} 个可用`:"—"}</span><strong>{completed?`${item.bestSpeed.toFixed(2)} MB/s`:"—"}</strong><div className="history-actions"><button disabled={!completed} title={completed?"":"未完成任务没有可载入的结果"} onClick={()=>loadHistory(item.runId)}>载入</button><button disabled={!completed} title={completed?"":"未完成任务没有完整参数快照"} onClick={()=>loadHistory(item.runId,true)}>重跑</button></div></div>})}{!filteredHistory.length&&<div className="empty">没有符合条件的历史任务</div>}</div>{filteredHistory.length>20&&<p className="history-limit">当前显示前 20 条，请缩小筛选范围。</p>}</section>}
      </>}

      {page==="nodes"&&<>
         <section className="hero small"><div><span>步骤 2</span><h1>生成优选订阅</h1><p>原始节点 + 优选 IP</p></div><div className="step"><em>1</em><i/><b>2</b></div></section>
        <section className="source-strip"><div><b>节点来源</b><span>{settings.panelUrl?`已配置 3X-UI${panelSessionActive?" · 已登录":panelTokenStored?" · Token 已安全保存":""}`:"未配置 3X-UI，也可以继续手工粘贴"}</span></div><div className="panel-fetch"><button className="ghost" disabled={loadingPanel||!settings.panelUrl||(!panelSessionActive&&!settings.panelToken&&!panelTokenStored)} onClick={loadPanelClients}>{loadingPanel?"正在读取…":"选择 3X-UI 客户端"}</button><button className="ghost legacy-fetch" disabled={loadingPanel||!settings.panelUrl||(!panelSessionActive&&!settings.panelToken&&!panelTokenStored)} onClick={fetchPanel}>读取全部节点</button></div></section>
        {panelClients.length>0&&<section className="panel client-picker"><div className="client-picker-head"><div><b>3X-UI 客户端</b><span>选择客户端后读取它关联的全部入站节点</span></div><select value={selectedPanelClient} onChange={e=>{setSelectedPanelClient(e.target.value);setPanelCandidates([])}}>{panelClients.map(client=><option key={client.email} value={client.email}>{client.email}{client.enabled?"":"（已停用）"} · {client.inboundIds.length} 个入站</option>)}</select><button className="ghost" disabled={!selectedPanelClient||loadingPanel} onClick={loadPanelClientLinks}>读取关联节点</button></div>{panelCandidates.length>0&&<><div className="candidate-list">{panelCandidates.map((candidate,index)=><label key={index}><input type="checkbox" checked={selectedPanelLinks.has(index)} onChange={()=>setSelectedPanelLinks(old=>{const next=new Set(old);next.has(index)?next.delete(index):next.add(index);return next})}/><b>{candidate.name||`${candidate.protocol} 节点`}</b><span>{candidate.protocol.toUpperCase()} · {candidate.transport||"tcp"} · {candidate.tls||"无 TLS"}</span><em>{candidate.address}:{candidate.port}</em></label>)}</div><div className="client-picker-actions"><small>例如 mt 下的 vmess-ws-tls-mt:443 会在这里单独列出。</small><button className="run" disabled={!selectedPanelLinks.size} onClick={useSelectedPanelLinks}>使用所选节点</button></div></>}</section>}
        <section className="panel compose"><div className="cols"><label><span>3X-UI 原始节点</span><textarea value={nodeLinks} onChange={e=>setNodeLinks(e.target.value)} placeholder="vless://…&#10;vmess://…&#10;trojan://…"/></label><label><span>优选入口</span><textarea value={preferredIps} onChange={e=>setPreferredIps(e.target.value)} placeholder="104.16.1.2#优选-1"/></label></div>
          <div className="compose-foot"><label><span>名称前缀（可选）</span><input value={namePrefix} onChange={e=>setNamePrefix(e.target.value)} placeholder="例如 CF"/></label><label className="safe-check"><input type="checkbox" checked={keepOriginalHost} onChange={e=>setKeepOriginalHost(e.target.checked)}/><span>保留 Host / SNI</span></label><button className="ghost" disabled={!nodeLinks.trim()} onClick={preview}>解析并预览</button><button className="ghost" disabled={!nodeLinks.trim()||!preferredIps.trim()} onClick={local}>本地生成</button><button className="run" title={!settings.enableOnline?"请先在设置中启用在线服务":!settings.baseUrl.trim()||!settings.generatePath.trim()?"请先填写在线发布服务":""} disabled={!settings.enableOnline||!settings.baseUrl.trim()||!settings.generatePath.trim()||publishing||!nodeLinks.trim()||!preferredIps.trim()} onClick={online}>{publishing?"生成中…":`生成在线订阅${nodePreview.length?` (${nodePreview.filter(v=>v.valid).length} 节点)`:""}`}</button></div>
        </section>
         {subscriptionHistory.length>0&&<section className="panel subscription-history"><div className="subscription-history-bar"><div className="subscription-history-heading"><details><summary>生成历史（{subscriptionHistory.length}）</summary><div className="subscription-history-list">{subscriptionHistory.slice(0,20).map(item=><div className="subscription-history-row" key={item.runId}><time>{new Date(item.createdAt*1000).toLocaleString()}</time><b className="subscription-history-mode">{item.mode==="online"?"在线":"本地"}</b><span className="subscription-history-count">{item.inputNodes} × {item.preferredEndpoints} = {item.outputNodes}</span><span className="subscription-history-formats" title={item.formats.join(" / ")}>{item.formats.join(" / ")||"—"}</span><em className="subscription-history-host" title={item.endpointHost||"本地生成"}>{item.endpointHost||"本地生成"}</em><strong className={item.status}>{item.status==="completed"?"完成":item.error||"失败"}</strong></div>)}</div></details><button className="history-clear" onClick={clearHistory}>清空历史</button></div></div></section>}
        {nodePreview.length>0&&<section className="panel"><div className="panel-title"><div><h2>节点预览</h2><p>UUID、密码和订阅凭据已隐藏</p></div></div><div className="node-grid"><div className="node-row head"><span>协议</span><span>名称</span><span>原地址</span><span>端口</span><span>TLS</span><span>传输</span><span>Host / SNI</span><span>状态</span></div>{nodePreview.map((v,i)=><div className="node-row" key={i}><b>{v.protocol}</b><span>{v.name}</span><span>{v.address}</span><span>{v.port}</span><span>{v.tls}</span><span>{v.transport}</span><span title={`${v.host||"—"} / ${v.sni||"—"}`}>{v.host||"—"} / {v.sni||"—"}</span><em className={v.valid?"ok":"bad"}>{v.valid?"有效":v.message}</em></div>)}</div></section>}
        {localResult&&<section className="panel output"><div className="panel-title"><div><h2>本地结果</h2><p>{localResult.inputNodes} 个节点 × {localResult.preferredEndpoints} 个入口 = {localResult.outputNodes} 个</p></div><div className="output-actions"><button onClick={()=>copy(localFormat==="links"?localResult.links.join("\n"):localFormat==="base64"?localResult.rawBase64:localFormat==="mihomo"?localResult.clashYaml:localResult.surgeProfile)}>复制</button><button onClick={()=>downloadText(localFormat==="mihomo"?"linkforge-mihomo.yaml":localFormat==="surge"?"linkforge-surge.conf":localFormat==="base64"?"linkforge-subscription.txt":"linkforge-nodes.txt",localFormat==="links"?localResult.links.join("\n"):localFormat==="base64"?localResult.rawBase64:localFormat==="mihomo"?localResult.clashYaml:localResult.surgeProfile)}>导出文件</button></div></div><div className="output-tabs"><button className={localFormat==="mihomo"?"active":""} onClick={()=>setLocalFormat("mihomo")}>Mihomo</button><button className={localFormat==="surge"?"active":""} onClick={()=>setLocalFormat("surge")}>Surge</button><button className={localFormat==="base64"?"active":""} onClick={()=>setLocalFormat("base64")}>Base64</button><button className={localFormat==="links"?"active":""} onClick={()=>setLocalFormat("links")}>节点链接</button></div><textarea readOnly value={localFormat==="links"?localResult.links.join("\n"):localFormat==="base64"?localResult.rawBase64:localFormat==="mihomo"?localResult.clashYaml:localResult.surgeProfile}/><details className="change-preview"><summary>查看地址替换差异（{localResult.changes.length}）</summary><div className="change-grid"><div className="change-row head"><span>协议</span><span>名称</span><span>原地址</span><span>优选地址</span><span>Host/SNI</span></div>{localResult.changes.slice(0,100).map((change,index)=><div className="change-row" key={index}><b>{change.protocol}</b><span>{change.name}</span><span>{change.originalAddress}</span><span>{change.preferredAddress}</span><em>{change.hostSniPreserved?"已保留":"已替换"}</em></div>)}</div></details>{localResult.warnings.length>0&&<details className="format-warnings"><summary>{localResult.warnings.length} 条格式兼容提示</summary><ul>{localResult.warnings.map((warning,index)=><li key={index}>{warning}</li>)}</ul></details>}</section>}
        {onlineResult&&<section className="panel output"><div className="panel-title"><div><h2>在线订阅</h2><p>{onlineResult.counts?.outputNodes!=null?`${onlineResult.counts.inputNodes??0} 个原节点 × ${onlineResult.counts.preferredEndpoints??0} 个优选地址 = ${onlineResult.counts.outputNodes} 个生成节点` : "复制链接或显示二维码"}</p></div></div><div className="subscription-cards">{subscriptionCardSpecs.map((card,index)=>{const value=String(onlineResult.urls?.[card.key]||"");return value?<article key={`${card.label}-${index}`}><div><b>{card.label}</b><span>{card.hint}</span></div><div><input readOnly value={value}/><button onClick={()=>copy(value)}>复制</button><button onClick={()=>showQr(card.label,value)}>二维码</button></div></article>:null})}</div>{Array.isArray(onlineResult.preview)&&onlineResult.preview.length>0&&<details className="online-preview"><summary>查看生成节点预览（前 {onlineResult.preview.length} 个，凭据已隐藏）</summary><div className="node-grid"><div className="node-row head"><span>协议</span><span>名称</span><span>地址</span><span>端口</span><span>TLS</span><span>传输</span><span>Host / SNI</span><span>状态</span></div>{onlineResult.preview.map((v:NodePreview,i:number)=><div className="node-row" key={i}><b>{v.protocol}</b><span>{v.name}</span><span>{v.address}</span><span>{v.port}</span><span>{v.tls||"—"}</span><span>{v.transport||"—"}</span><span title={`${v.host||"—"} / ${v.sni||"—"}`}>{v.host||"—"} / {v.sni||"—"}</span><em className={v.valid?"ok":"bad"}>{v.valid?"有效":v.message}</em></div>)}</div></details>}{Array.isArray(onlineResult.warnings)&&onlineResult.warnings.length>0&&<div className="online-warnings"><b>服务提示</b><ul>{onlineResult.warnings.map((warning:string,index:number)=><li key={index}>{warning}</li>)}</ul></div>}</section>}
      </>}

      {page==="settings"&&<section className="settings-wrap"><section className="hero small"><div><span>设置</span><h1>应用设置</h1><p>连接、发布与安全</p></div></section><section className="panel settings">
         <div className="settings-section panel-auth"><b>3X-UI 节点来源</b><label><span>面板 URL</span><input value={settings.panelUrl} onChange={e=>setSettings({...settings,panelUrl:e.target.value})} placeholder="https://example.com/secret/panel/"/></label>
          <div className="auth-tabs"><button className={panelAuthMode==="login"?"active":""} onClick={()=>setPanelAuthMode("login")}>账号登录</button><button className={panelAuthMode==="token"?"active":""} onClick={()=>setPanelAuthMode("token")}>API Token</button></div>
           {panelAuthMode==="login"?<><div className="login-fields" ref={panelLoginFieldsRef}><label><span>用户名 {panelLoginSaved?"· 已保存":""}</span><input autoComplete="username" value={panelLogin.username} onChange={e=>setPanelLogin({...panelLogin,username:e.target.value})}/></label><label><span>密码</span><div className="secret-input"><input type={showPanelPassword?"text":"password"} autoComplete="current-password" value={panelLogin.password} onChange={e=>setPanelLogin({...panelLogin,password:e.target.value})} placeholder={panelLoginSaved?"已安全保存；无需重新输入":""}/><button className="secret-toggle" type="button" aria-label={showPanelPassword?"隐藏密码":"显示密码"} title={showPanelPassword?"隐藏密码":"显示密码"} onClick={()=>setShowPanelPassword(!showPanelPassword)}><svg viewBox="0 0 24 24" aria-hidden="true">{showPanelPassword?<><path d="M3 3l18 18"/><path d="M10.6 10.6a2 2 0 0 0 2.8 2.8"/><path d="M9.9 5.3A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a18.2 18.2 0 0 1-3.1 3.9"/><path d="M6.1 6.1C3.5 8 2 12 2 12s3.5 7 10 7a10.9 10.9 0 0 0 4.1-.8"/></>:<><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/></>}</svg></button></div></label><label><span>2FA 验证码（启用时）</span><input inputMode="numeric" autoComplete="one-time-code" value={panelLogin.twoFactorCode} onChange={e=>setPanelLogin({...panelLogin,twoFactorCode:e.target.value})}/></label></div><label className="remember-login"><input type="checkbox" checked={rememberPanelLogin} onChange={e=>setRememberPanelLogin(e.target.checked)}/><span>保存到 Windows 凭据管理器，并在下次启动时自动登录</span></label><div className="credential-actions"><small>{panelSessionActive?"当前应用会话已登录；保存的密码仍可自动使用，但不会回显。":"密码不会写入设置、SQLite、localStorage 或日志。"}</small>{panelSessionActive?<button className="ghost" onClick={forgetPanelSession}>清除本机会话</button>:<><button className="run" disabled={loggingIn} onClick={loginAndOptionallySave}>{loggingIn?"正在登录…":panelLoginSaved?"更新账号并登录":"登录 3X-UI"}</button>{panelLoginSaved&&!panelLogin.password&&<button className="ghost" disabled={loggingIn} onClick={loginWithSaved}>使用已保存账号登录</button>}</>}{panelLoginSaved&&<button className="danger-link" onClick={removePanelLogin}>删除保存的账号</button>}</div></>:<><label><span>API Token {panelTokenStored?"· 已保存到 Windows 凭据管理器":"· 当前会话"}</span><div className="secret-input"><input type={showApiKey?"text":"password"} value={settings.panelToken} onChange={e=>setSettings({...settings,panelToken:e.target.value})} placeholder={panelTokenStored?"已安全保存；留空即可使用":""}/><button className="secret-toggle" type="button" aria-label={showApiKey?"隐藏 Token":"显示 Token"} title={showApiKey?"隐藏 Token":"显示 Token"} onClick={()=>setShowApiKey(!showApiKey)}><svg viewBox="0 0 24 24" aria-hidden="true">{showApiKey?<><path d="M3 3l18 18"/><path d="M10.6 10.6a2 2 0 0 0 2.8 2.8"/><path d="M9.9 5.3A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a18.2 18.2 0 0 1-3.1 3.9"/><path d="M6.1 6.1C3.5 8 2 12 2 12s3.5 7 10 7a10.9 10.9 0 0 0 4.1-.8"/></>:<><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/></>}</svg></button></div></label><div className="credential-actions"><small>适用于已在“设置 → 安全”启用 API Token 的新版面板。</small><button className="ghost" disabled={!settings.panelUrl||!settings.panelToken} onClick={storePanelToken}>安全保存 Token</button>{panelTokenStored&&<button className="danger-link" onClick={removePanelToken}>删除已保存 Token</button>}</div></>}
        </div>
          <div className="settings-section"><b>在线订阅服务</b><label className="toggle-setting"><input type="checkbox" checked={settings.enableOnline} onChange={e=>setSettings({...settings,enableOnline:e.target.checked})}/><span>允许在线生成订阅</span></label><div className="cols"><label><span>发布服务</span><input disabled={!settings.enableOnline} required value={settings.baseUrl} onChange={e=>setSettings({...settings,baseUrl:e.target.value,skipPublishWarning:false})}/></label><label><span>接口路径</span><input disabled={!settings.enableOnline} required value={settings.generatePath} onChange={e=>setSettings({...settings,generatePath:e.target.value,skipPublishWarning:false})}/></label></div><label><span>API Key（可选）</span><div className="secret-input"><input disabled={!settings.enableOnline} type={showApiKey?"text":"password"} value={settings.apiKey} onChange={e=>setSettings({...settings,apiKey:e.target.value})}/><button className="secret-toggle" type="button" disabled={!settings.enableOnline} aria-label={showApiKey?"隐藏 API Key":"显示 API Key"} title={showApiKey?"隐藏 API Key":"显示 API Key"} onClick={()=>setShowApiKey(!showApiKey)}><svg viewBox="0 0 24 24" aria-hidden="true">{showApiKey?<><path d="M3 3l18 18"/><path d="M10.6 10.6a2 2 0 0 0 2.8 2.8"/><path d="M9.9 5.3A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a18.2 18.2 0 0 1-3.1 3.9"/><path d="M6.1 6.1C3.5 8 2 12 2 12s3.5 7 10 7a10.9 10.9 0 0 0 4.1-.8"/></>:<><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/></>}</svg></button></div></label>{settings.skipPublishWarning&&<button className="restore-warning" onClick={()=>{setSettings({...settings,skipPublishWarning:false});setNotice("已恢复发送前提示；请保存设置。")}}>恢复发送前提示</button>}</div><p className="warning">在线订阅使用配置的 goodip 兼容服务；关闭后仍可本地导出。</p><div className="settings-actions"><button className="ghost" disabled={!settings.enableOnline||!settings.baseUrl.trim()||!settings.generatePath.trim()||testingService} onClick={testService}>{testingService?"测试中…":"测试连接"}</button><button className="run" onClick={save}>保存设置</button></div>
        <div className="diagnostics-actions"><button className="diagnostics-link" onClick={exportDiagnostics}>导出脱敏诊断包</button>{diagnosticsPath&&<><span title={diagnosticsPath}>{diagnosticsPath}</span><button className="ghost" onClick={()=>revealItemInDir(diagnosticsPath)}>打开所在文件夹</button></>}</div>
      </section></section>}
    </main>
  </div>
}
