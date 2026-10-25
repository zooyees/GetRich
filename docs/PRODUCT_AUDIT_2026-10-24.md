# GetRich 全页面产品与工程评估

评估日期：2026-10-24（Asia/Shanghai）

对象：当前工作区源码、Windows 0.1.0 发布程序、浏览器端脚本与 localhost API。

任务边界：检查与优化方案；本次未修改产品源码或正式数据库。

## 1. 结论

当前版本具备本地行情工作站的基本结构，但还不宜按“专业、稳定的日常看盘工具”验收。主要问题首先是数据可信度和状态一致性，其次是交互闭环和布局，最后才是主题与视觉细节。

本报告列出 36 项问题或能力缺口：12 项 P1、23 项 P2、1 项 P3。P1 包含已复现的错误行情质量、非法数据入库、列表与详情不一致、脏数据导致计算退出、桌面导入路径错误、跨进程配置覆盖，也包含代码确认的缩放崩溃、阻塞和后台调度风险。

优先级定义：

- P1：会导致错误数据显示、关键功能失败、崩溃或数据状态丢失，建议进入下一次发布前的修复范围。
- P2：有明确交互、可靠性、布局或扩展性缺陷，需要进入产品完善计划。
- P3：体验增强，基础正确性和稳定性完成后处理。

现有自动检查全部通过：核心 78 项测试、API 2 项测试，共 80 项；`cargo clippy --workspace --all-targets -- -D warnings` 通过。下面的缺陷仍能复现，说明现有测试主要覆盖函数级正确性，尚未充分覆盖完整用户流程、数据混合状态和故障恢复。

## 2. 检查方法与证据边界

| 检查方式 | 已检查内容 | 证据性质 |
| --- | --- | --- |
| Windows 原生界面操作 | 行情、股票详情、市场态势、数据、设置；空库和演示数据状态 | 实际截图与可访问性树观察 |
| 隔离 API 实测 | 导入、查询、质量、非法数据、月线、设置、双进程、事件连接饱和 | 运行结果，可复现 |
| Web 脚本状态验证 | 使用实际 `app.js`，通过 Node VM 和简化 DOM 驱动搜索、跳页、删除自选、图表坐标逻辑 | 脚本级复现，不等同于真实浏览器端到端测试 |
| 源码审查 | GUI、Web、服务、导入、数据库、实时计算、事件、配置、数据源、CI | 代码确认；未运行的场景单独说明 |
| 基线质量检查 | 工作区测试和严格 Clippy | 运行通过 |

隔离目录：`D:/windlink/windlink/GetRich/target/review-20261024`。API 测试使用 17881、17882 端口；正式用户数据未导入测试记录。故障注入仅用于隔离副本。

本次没有访问真实 Tushare 账号或权限接口，没有验证真实交易时段的全市场吞吐，也没有完成多显示器 DPI 矩阵、真实浏览器响应式截图矩阵和长时间运行压测。这些不能写成“已通过”，已纳入后续验收计划。

可重现记录：隔离目录内的 `audit-api.cjs`、`api-results.json`、`audit-web.cjs`、`web-results.json`、`audit-concurrency.cjs`、`concurrency-results.json`。该目录属于临时验证产物，关键结果也已写入本报告。

## 3. 页面覆盖与评价

| 页面 | 已有功能 | 主要缺口 | 建议验收状态 |
| --- | --- | --- | --- |
| 行情 | 市场板块、搜索、排序、自选、键盘选股 | 自选排序失效、行业过滤变成无报价占位、固定 400 条、选择与详情状态易失配 | 修复后复验 |
| 股票详情 / K 线 | 报价、日周月 K、均线、自选、资料 | 列表与详情报价口径不同、短序列缩放崩溃风险、悬停错位、桌面图表遮住指标、周期缓存过期 | 不通过 |
| 市场态势 | 市场截面、行业、自选、历史标记、缓存 | 新旧数据混合仍可判 good、小样本使用全市场口径、线程失效后继续呈现旧结果、跳转不等价于指标筛选 | 不通过 |
| 数据 | 文件导入、Tushare 同步、统计、Web 导入记录 | 桌面相对路径失败、同步阻塞、任务无进度取消、无效记录丢弃却成功、统计口径含糊 | 不通过 |
| 设置 | 默认板块、自选移除、数据源状态 | 缺配置入口与连接测试、Web 默认板块未用于初始化、保存反馈不足、跨进程配置覆盖 | 修复后复验 |
| 公共框架 | localhost API、SQLite WAL、事件通知、后台计算 | 连接资源隔离、事件恢复、统一读模型、任务调度、错误展示和 Windows 自动化覆盖不足 | 修复后复验 |

## 4. P1：优先修复的问题

### F01：一只有新行情即可掩盖整批旧行情，质量被误报为 good

**已实测。** 隔离库有 104 只 A 股报价，其中 1 只为当前交易日，其余 103 只为 2024 年历史数据。`/v2/pulse` 返回 `stale=false`、`quoted_valid=104`、`coverage=1`、`fresh_ratio_3x=1`、`level=good`。

根因：用 `MAX(ts)` 和 `MAX(ingested_at)` 判断整批新鲜度，`tape::classify` 仍把所有行加入市场统计；质量计算又把整批股票计为有效。导入时间也不能代表行情源时间。

方案：按每只股票的市场、交易日、源时间、接收时间、停牌状态判断有效性；分开输出 observed、valid、stale、missing、invalid。市场广度、行业与金额只能使用同一目标交易日的有效截面。历史浏览应另有明确的数据日期模式。

验收：上述样本的有效数量必须为 1；历史 103 只不能进入当前有效覆盖率；不能评级为 good。盘中同日延迟与跨交易日历史数据必须呈现不同原因。

位置：[pulse/mod.rs:362](D:/windlink/windlink/GetRich/crates/getrich-core/src/pulse/mod.rs:362)、[pulse/tape.rs:71](D:/windlink/windlink/GetRich/crates/getrich-core/src/pulse/tape.rs:71)、[realtime.rs:182](D:/windlink/windlink/GetRich/crates/getrich-core/src/realtime.rs:182)。

### F02：报价没有领域校验，可出现负价格、负成交额和方向矛盾

**已实测。** 导入 `last=-10, prev_close=10, high=9, low=11, volume=-100, turnover=-1000, change_pct=20`，接口返回成功。详情实际返回涨跌额 -20、涨跌幅 +20%、方向 down、振幅 -20%。市场态势又优先信任 `change_pct`，会把这条数据作为上涨统计。

方案：在所有导入入口共用 QuoteValidator，验证数值范围、有限值、OHLC 关系、量额非负、价格与涨跌幅一致性；保留原始供应商字段与校验结果，统一决定可信的派生字段。无效行进入隔离记录，不参与价格、排序和聚合。

验收：非法样本被拒绝或隔离，返回明确行号与字段；同一标的的颜色、涨跌额、涨跌幅、市场广度一致。

位置：[import.rs:542](D:/windlink/windlink/GetRich/crates/getrich-core/src/import.rs:542)、[view.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/view.rs)、[pulse/tape.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/pulse/tape.rs)。

### F03：同一股票在列表和详情显示不同价格

**已实测。** 旧报价为 10，后续日 K 收盘为 12：`/v1/quotes` 显示 10，`/v1/stocks/...` 显示 12。列表直接读取 quotes，详情另用 `prefer_session_last` 替换价格；GUI 还会局部替换列表行，导致不同入口的排序、颜色和价格可能失去一致性。

方案：在核心建立唯一的报价选择规则和 CanonicalQuote 读模型，由列表、详情、排序、市场态势共同使用；UI 只格式化数据。使用明确的源类型、源时间、交易日、最终性和修订号决定优先级。

验收：同一数据版本下列表、详情、K 线对应的收盘/最新口径有清楚定义且一致；排序应基于实际显示值。

位置：[service.rs:362](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:362)、[service.rs:450](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:450)、[view.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/view.rs)。

### F04：普通脏数据能够触发 panic，计算停止后仍显示已启动

**已实测。** `listing_date="中文ab"` 可成功导入。该字符串 UTF-8 长度为 8 字节，涨跌停规则按字节切 `[0..4]`，CLI `pulse` 直接 panic：`end byte index 4 is not a char boundary`。后台实例在导入后停止产生新快照，接口仍返回 `worker_started=true`、`compute_failures=0` 和上次的 good 质量。

方案：日期必须使用日期解析器，禁止对外部日期字符串按假定字节位置切片；导入时验证日期。计算线程增加退出检测、心跳、最后成功时间、失败原因和恢复机制；读接口按快照年龄降级。`started` 与 `alive/healthy` 必须分开。

验收：任意字符串日期不能使进程或工作线程退出；计算失败后 UI 有故障状态，质量不能继续保持 good；修正输入后可恢复。

位置：[pulse/limit.rs:14](D:/windlink/windlink/GetRich/crates/getrich-core/src/pulse/limit.rs:14)、[realtime.rs:87](D:/windlink/windlink/GetRich/crates/getrich-core/src/realtime.rs:87)、[realtime.rs:118](D:/windlink/windlink/GetRich/crates/getrich-core/src/realtime.rs:118)。

### F05：桌面 K 线少于 8 根时，缩放路径会 panic

**代码确认，未通过 UI 故意触发崩溃。** 缩放使用 `clamp(8, n)`；当周线、月线或新股数据只有 1–7 根时，下限大于上限。演示数据合成月线就可能进入短序列条件。

方案：缩放下限取 `min(8, n)`，全程处理空序列与单根序列；将可视范围计算提取为独立函数。

验收：0、1、2、7、8、80、800 根分别执行放大、缩小、切周期，不能 panic，范围始终合法。

位置：[GUI app.rs:2094](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:2094)、[GUI app.rs:2102](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:2102)。

### F06：桌面导入、网络同步和部分数据库查询在 UI 线程执行

**代码确认，未执行真实网络超时测试。** `ui_data` 内直接调用 `ingest_tushare`、`import_file`；详情手动拉取直接调用 `sync_tushare_bars`。列表、详情和切周期也同步读库。数据库 busy_timeout 为 5 秒，普通网络客户端超时至少 90 秒；这些等待可能冻结窗口。

方案：UI 只提交任务；后台任务服务执行导入、网络和较重查询，通过结果消息更新页面。任务具备 queued/running/succeeded/partial/failed/cancelled 状态，UI 有忙碌反馈和取消入口。

验收：网络超时、大文件、数据库锁等待期间，窗口可移动、页面可切换、取消按钮可响应；不允许同步工作占用 UI 线程。

位置：[GUI app.rs:1621](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:1621)、[GUI app.rs:1638](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:1638)、[GUI app.rs:1163](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:1163)、[tushare.rs:135](D:/windlink/windlink/GetRich/crates/getrich-core/src/tushare.rs:135)。

### F07：薄数据自动拉取可反复启动，返回结果没有周期身份校验

**代码确认。** GUI 按当前页面 K 线数量 `<40` 判断需要拉取；周/月线自然较少。每次收到返回就清除 fetching，下一轮 update 又可能拉取。未配置 token 的错误被忽略后，仍可能读取并回传空/薄数据，再次开始。后台结果只比对股票，不比对 timeframe 和请求版本，切周期后旧结果可能覆盖新周期。

方案：补全判断基于日线覆盖日期和目标历史区间；请求键包含 instrument、timeframe、range、generation。失败使用退避和显式重试，配置缺失应停止自动拉取；返回结果必须匹配当前请求身份。

验收：周/月线少于 40 根不等于持续拉取；未配置 token 不产生请求循环；快速切日/周/月只显示最终所选周期。

位置：[GUI app.rs:620](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:620)、[GUI app.rs:756](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:756)。

### F08：交易日缓存缺少有效期，过期日历可被当成当天权威结果

**代码确认。** `trading_day` 读取缓存的最后开市日，直接与今天比较，不检查缓存的 fetched_at/as_of 是否覆盖今天。跨日启动或日历更新失败时，可能把正常交易日判成非交易日，并继续使用旧交易日计算。

方案：CalendarService 以市场和日期区间管理日历，记录有效覆盖范围与获取时间。过期缓存应显示日历未知/降级，不能作为当天非交易日的确证。按市场分别处理 A/H/US 的时区与交易日。

验收：前一日缓存、节假日后首次启动、无网络、日历请求失败都能明确区分；不把过期缓存标成有效 trade_cal。

位置：[pulse/mod.rs:306](D:/windlink/windlink/GetRich/crates/getrich-core/src/pulse/mod.rs:306)、[ingest.rs:101](D:/windlink/windlink/GetRich/crates/getrich-core/src/ingest.rs:101)。

### F09：限流和退避保存在短生命周期客户端内，可被下一次请求重置

**代码确认，未对真实供应商制造限流。** 每次轮询、单股刷新都会调用 `tushare_client` 新建 Client；TokenBucket 和 Backoff 都在 Client 内。全市场轮询、GUI 单股和 Web 每个页面的请求因此没有共同的账号预算。

方案：按数据源账号持有共享客户端、连接复用、限流器和退避状态；所有入口进入同一调度队列。页面订阅本地缓存，避免多个页面各自触发相同的远程刷新。

验收：多个标签、多个标的与全市场刷新并行时，仍遵守同一预算；供应商限流后所有同账号请求共同退避。

位置：[service.rs:214](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:214)、[ingest.rs:158](D:/windlink/windlink/GetRich/crates/getrich-core/src/ingest.rs:158)、[tushare.rs:112](D:/windlink/windlink/GetRich/crates/getrich-core/src/tushare.rs:112)。

### F10：GUI/CLI 多进程保存会互相覆盖自选

**已实测。** 两个进程共用同一配置，A 添加 601600，B 添加 601601；磁盘最终只保留 B 的 601601，A 仍在内存中认为 601600 已保存。

根因：当前 Mutex 只保护同一个 App 实例。虽然文件替换是原子的，每个进程仍用旧内存里的完整自选列表覆盖配置。

方案：单服务拥有设置写入权，GUI/CLI 使用同一服务；或实现跨进程锁、版本比较和读取后合并增量。将“添加/移除一个自选”作为持久化操作，避免全列表覆盖。

验收：双进程并行添加后两项都存在；移除和添加冲突有确定规则；各端可收到 settings.changed 并同步。

位置：[service.rs:687](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:687)、[service.rs:739](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:739)。

### F11：桌面默认示例路径依赖进程工作目录，文件存在也导入失败

**已实测。** 隔离程序旁存在 `examples/demo.board.json`，API 相对路径导入成功；GUI 同样路径却返回 `The system cannot find the path specified`。GUI 直接把相对路径交给文件系统，API 则按 data root 解析。

方案：所有入口共享 ImportPathResolver；相对路径统一基于数据根目录。桌面提供文件选择，Web 根据部署模式明确“服务端文件”和“上传文件”，默认示例应有直接入口。

验收：资源管理器双击、快捷方式、不同工作目录启动都可导入默认示例；路径不存在时显示实际解析路径和解决方式。

位置：[GUI app.rs:1638](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:1638)、[invoke.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/invoke.rs)。

### F12：已保存的周/月线不会随日线更新而失效

**已实测读模型行为，并由代码确认更新路径。** 同一月份的最新日线 close=12，库内已有旧月线 close=10，月线接口继续返回 10。`list_bars` 只在该周期完全没有记录时才从日线合成；Tushare 初次拉取会保存周/月线，之后热更新只改日线，衍生周期因此可能长期滞后。

方案：以日线为基础重算受影响周/月，或给聚合记录附依赖版本并在读取时验证。保留是否为未完成周期、基础日线范围和计算版本。

验收：日线新增或修订后，对应周/月 close、high、low、volume、turnover 更新；历史修订也能重算；当前周/月有“未收完”标记。

位置：[service.rs:567](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:567)、[tushare.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/tushare.rs)、[resample.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/resample.rs)。

## 5. P2：功能、交互、布局和可靠性问题

| ID | 问题与证据 | 优化方案与验收标准 |
| --- | --- | --- |
| F13 | **导入结果口径失真，已实测。** 不可能的 K 线被跳过，`bars_upserted=0`、`errors=[]`、`status=ok`；更早的报价被拒写，却报告 `quotes_upserted=1`。GUI 对 partial 也只展示完成数量。 | 共用 WriteOutcome，报告 requested/inserted/updated/unchanged/stale/invalid/failed；无效 K 线必须有行级错误。验收：重复和陈旧输入不能计为新增/更新；partial 不显示全成功。 |
| F14 | **搜索跨页改变路由，脚本已复现。** 在设置页输入顶部搜索，`state.view` 从 settings 变为 list；桌面搜索延迟回调也会打开股票。 | 搜索限定在行情页，或设计明确的全局搜索结果浮层；搜索输入和路由策略共用。验收：非行情页面不会因为普通输入而无提示跳页。 |
| F15 | **延迟刷新把用户拉回态势页，脚本已复现。** 先安排 pulse debounce，再切设置，回调调用 `loadPulse`，强制 `state.view=pulse`。 | 路由退出取消定时器；刷新函数只刷新模型，不负责导航；回调检查路由代次。验收：离开页面后任何在途请求/定时器均不能改变当前路由。 |
| F16 | **设置页移除自选后离开设置，脚本已复现。** board=WATCH 时，移除回调先 `loadSettings` 再 `loadList(false)`，最终变成 list。 | 更新自选模型和依赖列表缓存，不调用导航函数。验收：移除后保留设置页滚动位置，并有成功/失败反馈。 |
| F17 | **选择与详情没有完整一致性规则。** 无匹配过滤结果仍保留旧股票详情（脚本已复现）；Web 用行号保存选中位置，排序后没有按 instrument key 恢复，可能高亮一只而详情显示另一只。 | 使用 `selectedInstrumentKey`，筛选后选择保留/清空有固定规则；列表空时显示详情空态或明确“当前筛选外的股票”。验收：搜索、排序、刷新和移除后高亮、标题、星标、K 线始终对应同一标的。 |
| F18 | **默认板块设置不闭环。** Web 初始 `state.board=ALL`，启动未读取默认设置；设置保存无成功提示/错误恢复；API 可保存 INVALID（已实测），GUI 吞掉保存错误。 | 初始化载入偏好，持久化值使用枚举校验；保存后反馈，失败回滚控件。默认值与当前浏览状态分开。验收：重开 Web/GUI 都使用相同默认值，非法值返回 400。 |
| F19 | **K 线悬停和均线受视窗影响。** 10 根数据时指向第一根，Web 实际选中第 10 根（脚本已复现）：绘制补了 70 个空槽，命中测试没减去补白。两端均线基于可见切片重算，缩放会改变可见左侧同一天的均线值。 | 绘制和命中共用坐标模型；在完整序列计算指标，再裁切视窗。验收：首/中/末根命中正确，同一日期均线不因缩放改变。 |
| F20 | **桌面图表占满剩余高度，关键报价项被挤出视口；缺少数值坐标。** 1440×900 级窗口实际观察到下方今开/最高/量额等指标不可见。`allocate_painter(ui.available_size())` 消耗剩余空间；图内只有蜡烛、成交柱和文本，没有价格轴/日期轴。 | 用固定底部摘要区和可伸缩图表区；图表单独分配高度，详情可滚动；增加价格、时间、量轴和图例数值。验收：1366×768 与 1440×900 都可看到核心指标，无需猜测价格尺度。 |
| F21 | **Web 小窗口布局没有真正收敛。** 行情始终双栏；两列最小 300+360、间距 12，再加外边距，720 以下媒体规则仍不改双栏。顶部导航、搜索和长状态条不折行，行业行也有固定列宽。此项为代码确认，未完成真实浏览器尺寸矩阵。 | 大屏双栏、窄屏单栏详情路由；状态条允许折行/压缩，行业卡片按断点重排。验收：390、768、1024、1440 宽度无整页横向溢出，数据表自身横滚有提示。 |
| F22 | **固定前 400 条，没有继续浏览入口。** Web/GUI 均请求 limit=400，虽提示截断，却没有下一页或增量加载。全市场用户不能完整遍历。 | 服务端稳定分页/游标和虚拟列表；显示筛选总数、已加载数，稳定排序含 market+symbol。验收：可遍历 6000+ 标的，不重复、不遗漏，不需先猜代码搜索。 |
| F23 | **自选排序和行业筛选失效，API 已实测。** `sort=change_pct` 仍按 config 自选顺序返回；`industry=Review` 返回 6 个无报价占位，而非匹配行业。合并函数把 SQL 排序/过滤后的结果再次按完整自选名单重建。 | 先统一得到自选读模型，再执行一致筛选和排序；区分缺 listings 与被筛选排除。验收：自选与全部板块的排序/行业语义一致，排除项不能变成幽灵行。 |
| F24 | **局部样本用“全市场”口径，质量信息没有前置。** 只有 4 只 A 股的演示库，桌面仍显示“全市场成交额”；v2 的 universe_total 也只是本地 listings 数量，不是供应商完整市场基数，页面仍调用 v1。 | 显式提供 universe_scope、expected_total、available、valid、coverage、source_lag、limitations；不足全市场时使用“已导入样本”。验收：少量导入数据绝不使用全市场事实口吻。 |
| F25 | **态势指标点击不等价于指标筛选。** 点击“下跌”只是跳 CN 按涨跌幅升序排序，仍含平盘和上涨；“上涨”“涨停”也只是排序。 | 提供 change_sign、limit_state 等可组合过滤，并显示筛选条件条；不支持筛选的指标不伪装成可下钻按钮。验收：结果集合能与指标定义一致，数量差异有时间/样本解释。 |
| F26 | **“刷新态势”只读缓存，失败时页面残留旧板块。** 手动刷新不能要求后台重算；`loadPulse` 异常仅清标题/会话/部分 tape，行业、自选等旧内容可保留。缓存过旧也缺独立状态。 | 明确区分刷新视图、重新计算、更新源数据；采用 last-good-data + stale/error 标记，全部区块绑定同一 snapshot_id。验收：失败时旧数据统一标注，不能出现新错误标题配旧成功区块。 |
| F27 | **数据源设置只能看，不能完成接入。** 实际设置页只有状态文字；未配置 token 时同步仍可点击，美股详情也有 Tushare 按钮。文字指示手工改 config，但没有路径或校验入口。 | ProviderCapabilities 驱动按钮可用性；配置表单、掩码、保存、连接测试、权限与支持市场说明形成闭环。验收：未配置/无权限/不支持市场时给出明确动作和原因，不进入注定失败的任务。 |
| F28 | **数据任务缺少可管理的生命周期。** Web 连续点击可发起多个同步/导入，没有作业 ID、进度、取消和去重；客户端超时不代表服务端停止。GUI 没有导入记录列表，Web 只展示最近 10 条和首个错误。 | 统一 JobService；按任务键去重，展示进度、阶段、耗时、错误详情和重试。支持校验预览、确认应用、失败重试与取消。验收：重复点击只有一个任务，超时后可查询最终结果。 |
| F29 | **数量、时间、单位表达不统一。** 数据页“日线”取全部 bars 数，包含周/月等周期；Web 导入记录直接截 UTC 字符串，不同于顶部上海时间；详情只显示日期，无法判断盘中延迟；多市场使用统一时区呈现缺解释。 | 分开统计各周期；同时显示市场交易时间、接收时间、时效状态；共享格式化规范，金额/成交量明确币种和单位。验收：同一记录在各页时间一致且带时区，统计标签与查询条件相符。 |
| F30 | **事件长连接与普通 API 共用 64 个请求名额。** 已实测 64 个 `/v1/events` 连接后 `/v1/health` 返回 503。每个连接长期占一个线程/槽位，缺少独立预算。 | 流连接独立限额和执行资源，健康及控制接口保留预算；设置请求读写时限，限制慢连接，避免接受循环被拒绝响应阻塞。验收：流连接达到上限，健康检查与常规读接口仍可用。 |
| F31 | **事件序号缺少服务实例身份与可靠的恢复协议。** 序号进程重启归零，Web 只取旧值和新值的 max；hello/ping 也推进恢复游标。服务端在取得 state 锁之前分配 seq，多线程提交可能让历史与投递顺序不同；超过回放窗口没有 resync 提示。代码确认。 | 增加 stream_epoch，序号在同一临界区分配和发布；只有已处理的业务事件推进游标。溢出/重启返回 resync_required 并拉完整快照。验收：重启、断线、乱序、回放溢出后最终状态一致。 |
| F32 | **快照版本不保证与实际读取内容来自同一数据库截面。** 计算、读取 latest_batch_version、统计 universe 为多个自动提交查询；并发写入可让旧内容带新版本。日期/phase/设置变化又可能共用同一批次版本；启动先发布持久化快照而未检查年龄。代码确认。 | 在一致读事务读取输入和 data_version；snapshot_revision 与 data_version 分离，附 config_version、calendar_version 和 computed_at。启动缓存需验证日期/年龄。验收：每个快照可追溯输入版本，重启和并发导入不发布版本错配。 |
| F33 | **启动错误和服务归属未向用户说明。** GUI 配置解析失败回退默认，迁移错误仅日志记录，API 绑定失败后仍继续显示。隔离 GUI 的日志实测记录 7878 端口占用，窗口仍正常出现；此时该端口可能属于另一数据目录。 | 启动诊断明确配置/DB/服务状态；单实例或绑定协商，校验 instance_id 与 data_root；失败呈现恢复入口。验收：不能静默换配置，也不能让桌面与浏览器连接到不同实例而无提示。 |
| F34 | **新增页面/数据源需要修改多处巨型实现。** Web 状态、路由、请求、图表、页面、事件混在一个 app.js；GUI 同样集中于 app.rs。部分报价选择还在 UI 层处理，invoke 字符串分发和 v1/v2 原始/信封格式并存。 | 先按业务拆页面与状态，再提取共享核心规则；请求/错误/任务/订阅服务独立。数据源通过能力接口接入，DTO 与错误契约版本化。验收：新增只读工具页不需修改报价或导入内部实现，新 provider 不进入现有页面条件分支。 |
| F35 | **质量门禁缺少用户流程和 Windows 覆盖。** 现有 80 项测试通过，但 GUI/CLI 测试二进制为 0 用例；CI 仅 ubuntu，Web 无路由/交互回归。表格选择与排序、K 线 tabs/canvas 的键盘和读屏语义也不足。 | 增加跨端契约、Web 实际浏览器测试、Windows GUI 烟测和故障注入；验证焦点、aria-selected/aria-sort、色弱识别与图表文本替代。验收：已发现的关键缺陷均有能失败再通过的回归场景，Windows 编译和发布产物也受检。 |

### P2 主要源码定位

- 导入结果：[import.rs:417](D:/windlink/windlink/GetRich/crates/getrich-core/src/import.rs:417)、[db/bars.rs:21](D:/windlink/windlink/GetRich/crates/getrich-core/src/db/bars.rs:21)。
- 路由与选择：[app.js:590](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:590)、[app.js:930](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:930)、[app.js:1002](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:1002)、[app.js:1129](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:1129)。
- 图表坐标与均线：[app.js:498](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:498)、[app.js:1192](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.js:1192)、[GUI app.rs:2062](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:2062)。
- 布局：[app.css:151](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.css:151)、[app.css:675](D:/windlink/windlink/GetRich/crates/getrich-api/web/app.css:675)、[GUI app.rs:1017](D:/windlink/windlink/GetRich/crates/getrich-gui/src/app.rs:1017)。
- 自选筛选/排序：[service.rs:886](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:886)。
- 设置与启动：[service.rs:727](D:/windlink/windlink/GetRich/crates/getrich-core/src/service.rs:727)、[GUI main.rs:87](D:/windlink/windlink/GetRich/crates/getrich-gui/src/main.rs:87)。
- 事件资源/恢复：[http.rs:23](D:/windlink/windlink/GetRich/crates/getrich-api/src/http.rs:23)、[http.rs:308](D:/windlink/windlink/GetRich/crates/getrich-api/src/http.rs:308)、[events.rs:99](D:/windlink/windlink/GetRich/crates/getrich-core/src/events.rs:99)。
- 快照一致性：[realtime.rs:150](D:/windlink/windlink/GetRich/crates/getrich-core/src/realtime.rs:150)、[db/realtime.rs](D:/windlink/windlink/GetRich/crates/getrich-core/src/db/realtime.rs)。
- 自动化门禁：[ci.yml](D:/windlink/windlink/GetRich/.github/workflows/ci.yml)。

## 6. P3：F36 个性化与视觉完成度

在前述问题修复后，再增加涨跌颜色方案、明暗主题、列表列显隐/顺序、布局宽度记忆、快捷键面板、近期查看和工作区预设。当前设置页更像静态说明，适合重构成真正可配置的控制面板。

验收应围绕“偏好可以修改、立即生效、重启保持、两端定义一致”，避免只添加没有行为的控件。

## 7. 页面优化方案

### 7.1 公共框架

顶部保留四个清晰入口。搜索只在行情工作区展示，或作为有明确结果浮层的全局工具。状态区拆成连接状态、数据源状态和行情时效，不能用一段“已导入 N 只 + 最大时间”代替所有信息。

统一页面状态：initial、loading、ready、empty、partial、stale、error、offline。保留上次成功数据时必须同时显示该数据时间和失败原因；局部错误不能把其他内容伪装成当前成功结果。

为每个操作统一提供请求中状态、重复提交保护、成功反馈、可理解的失败说明和可执行的恢复入口。技术错误详情可以展开查看，主提示使用用户能完成的动作。

### 7.2 行情与详情

- 大屏采用可调整宽度的列表与详情双栏；列表保留筛选条件条、清除条件、结果数量、稳定排序和增量加载。
- 选择状态只保存 instrument key。股票列表刷新后恢复同一标的；不存在时按约定清空或选择第一项，不用旧行号推测。
- 详情上方展示价格、币种、市场交易时间、数据状态和自选；图表区可伸缩，下方价格摘要固定可见。
- 图表补齐价格轴、日期轴、量轴、十字线数值与缩放重置；日/周/月明确周期，未完成周期要标记。
- 指标先基于完整数据计算，视窗只负责裁切；除权口径需明示，未来接复权行情时不能把不同口径直接叠加。
- 小窗口使用单栏列表/详情路由；详情返回后恢复列表条件和滚动位置。

### 7.3 市场态势

第一层呈现目标市场、交易日、数据截至时间、有效样本/预期样本、质量和延迟。当前时钟退居辅助位置，历史快照使用显著的历史模式标识。

第二层展示广度、金额、涨跌停估算等核心指标，并允许查看统计定义。局部导入库显示“样本截面”；只有 universe 经过完整性验证后才使用全市场描述。

第三层展示行业、自选相对表现和已接入的其他数据。未接入能力放在简短说明中；`fallback`、`P0 未接入`、“禁止把昨日数字说成实时”等内部措辞应转成“工作日日历估算”“该数据源尚未启用”等产品说明。

所有可点击指标需与结果过滤条件等价；跳转后显示来源条件和快照日期。历史指标如果只能跳到当前列表，必须明确提示日期差异，或支持历史列表查询。

### 7.4 数据

按导入文件、同步数据源、任务记录、库健康四块组织。首次使用可以直接选择演示数据、选择文件或配置数据源。

导入流程为：选择文件 → 校验预览 → 显示将新增/更新/跳过/拒绝的记录 → 应用 → 查看结果。允许用户查看全部错误，不以首个错误或总数代替诊断。

同步流程为：能力/权限检测 → 选择范围 → 提交任务 → 查看阶段进度 → 完成/部分完成/失败。任务可取消、可追踪、可重试，数据页与设置页共享数据源状态。

统计应分开 listings、quoted listings、各周期 bars、目标交易日有效报价和数据库占用。增加版本、最后成功任务、数据源水位和异常数量，便于解释数据为什么没更新。

### 7.5 设置

建议分为数据源、存储、行情与图表、外观与快捷键、诊断五组。先实现数据源配置/测试、默认板块、存储路径、导出诊断和更新状态；可选主题放在后续。

数据源连接测试与保存配置分开；已保存、正在连接、权限不足、离线、交易时段外都应有明确语义。自选管理支持搜索添加、移除、批量导入/导出和排序；空态提示必须指向真实存在的入口。

## 8. 稳定性与扩展性结构方案

建议保留当前 Rust/SQLite 技术基座，先收敛职责和数据契约，不以整套重写作为前提。

```text
GUI / Web / CLI
  ├─ 页面状态、格式化、布局、操作入口
  └─ 统一应用服务
       ├─ QuoteReadModel：唯一报价选择与派生字段
       ├─ MarketSnapshotService：同一读事务、质量、版本、快照
       ├─ JobService：导入/同步/重算、进度、去重、取消
       ├─ SettingsService：版本、增量更新、跨进程一致性
       ├─ ProviderRegistry：能力、共享客户端、预算、权限
       └─ CalendarService：市场时区、覆盖范围、有效期
             └─ Repository / 单写入调度 / SQLite
```

需要稳定的契约至少包括：

| 契约 | 必要字段或规则 |
| --- | --- |
| InstrumentKey | market + symbol；不能依赖显示名或列表索引 |
| CanonicalQuote | trade_date、source_at、received_at、source_kind、finality、revision、validation、data_version |
| Snapshot | snapshot_revision、data_version、config_version、calendar_version、computed_at、quality、universe_scope |
| Job | id、kind、scope、status、progress、counts、errors、started_at、finished_at、idempotency_key |
| Event | stream_epoch、seq、type、entity_key、version；支持 resync_required |
| Error | code、用户消息、可重试性、字段/行级细节；认证/权限/限流/输入/存储/网络分开 |

快照计算在一个一致读事务中获取输入与数据版本；存储失败不能发布成成功快照。慢客户端和事件订阅不应拖住写入或健康接口。

GUI/CLI 需明确谁拥有后台服务：首选同一数据目录只运行一个服务实例，其他界面附着到它。若支持多进程直接读写，就必须补跨进程设置同步、任务去重和服务归属检测，不能仅依赖进程内 Mutex。

## 9. 分阶段落地顺序

| 阶段 | 交付内容 | 出口条件 |
| --- | --- | --- |
| A：数据可信与崩溃修复 | F01–F13；统一校验/报价选择、逐行新鲜度、日期安全、缩放边界、周期失效、导入真实结果 | 所有关键反例转成回归测试；错误数据不能参与当前市场分析 |
| B：操作闭环与任务调度 | F14–F18、F22–F23、F27–F30、F33；统一路由状态、偏好、分页、能力、作业、实例归属 | 快速切页/切股/切周期、重复点击、离线与超时均不丢失状态或冻结 UI |
| C：布局与专业表达 | F19–F21、F24–F26、F29、F36 中必要偏好；图表、样本/全市场口径、状态层级 | 指定窗口与 DPI 矩阵无核心控件遮挡；指标定义和跳转结果一致 |
| D：持续运行与扩展门禁 | F31–F35；事件恢复、快照版本、模块拆分、Windows/Web 自动化、长时间运行 | 故障恢复和长时间运行验证通过；新增页/源只扩展对应模块与契约 |

阶段 A 和 B 的核心稳定性问题应优先于更换主题、增加更多指标或接入更多数据源。每阶段按验收项关闭问题，避免把“代码能编译、包能启动”作为产品完成标准。

## 10. 验收矩阵

| 维度 | 必测场景 | 通过标准 |
| --- | --- | --- |
| 数据状态 | 空库、仅 listings、仅报价、仅日线、混合日期、负值、错误日期、重复、陈旧、修订 | 状态可解释，统计准确，无 panic，无虚假成功 |
| 行情一致性 | 列表/详情/日周月/市场态势/排序，同版本并行读取 | 同一口径和版本，派生字段一致，周期更新及时 |
| 请求竞争 | 连续搜索、排序、切股、切周期、切页、取消、超时后返回 | 只应用匹配当前请求身份的结果，不抢路由 |
| 配置 | 保存失败、重启、两个进程、手工编辑、非法枚举、损坏 JSON | 不静默覆盖，错误可恢复，状态跨端一致 |
| 外部数据 | 无 token、无权限、限流、DNS/网络失败、响应格式错、部分市场不支持 | 能力控制正确，共享预算，有退避和真实结果 |
| 交易时间 | 开盘前、竞价、盘中、午休、收盘、周末、节假日、跨日、过期日历 | 交易日、源时间与状态一致，不用旧缓存冒充当天事实 |
| 故障恢复 | 工作线程退出、数据库锁、持久化失败、服务重启、断线超过回放窗口 | 健康状态及时降级，重连可全量恢复，不长期显示旧 good |
| 资源 | 多标签事件订阅、大列表、大文件、重复任务、慢连接 | 有明确上限，控制与健康接口不被挤占 |
| UI 布局 | 1366×768、1440×900、1920×1080；Windows 100/125/150/200%；Web 390/768/1024/1440 宽 | 核心信息可见，无整页溢出、控件覆盖或不可达操作 |
| 图表 | 0/1/2/7/8/80/800 根，缩放、首尾悬停、日周月切换 | 范围合法、数值命中正确、均线不受缩放影响 |
| 可访问性 | 键盘导航、焦点顺序、读屏、非颜色方向标记 | 可独立完成选股、查看、切周期、自选和恢复错误 |
| 发布 | Windows release、干净解压目录、不同工作目录启动、端口冲突 | 示例可用，路径一致，错误可见，版本和实例可辨认 |

## 11. 当前应保留的基础

已有 SQLite WAL 和迁移、板块查询、空态基础、幂等自选入口、导入事件、有限事件队列、后台快照、历史数据显式提示，以及核心/接口测试。这些可作为整改基座。

上次修复使“整批都是旧行情”的情形能被识别，但此次混合日期反例说明时效模型还需要从整批布尔值升级到逐标的有效性。原子替换配置也解决了单次写入中断问题，但跨进程的增量合并仍需补齐。后续应在这些基础上完善闭环，而不是把已有防护误认为所有相关场景都已覆盖。
