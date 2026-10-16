# Greenlife 前端

Vue 3 + TypeScript + Vite 的单页应用。安装 / 运行 / nginx 部署见[根目录 README](../README.md#前端)。

```bash
npm install
npm run dev     # http://localhost:5173
npm run build   # vue-tsc 类型检查 + 打包到 dist/
npm run lint    # eslint 检查（有错时退出码 1，CI 可用）
npm run lint:fix
npm run format  # prettier 格式化（配置在 .prettierrc.json）
```

### 工具链分三块，职责不重叠

| 命令             | 工具                           | 管什么                                                       |
| ---------------- | ------------------------------ | ------------------------------------------------------------ |
| `npm run build`  | `vue-tsc`                      | 类型与语法（含模板里的类型错误）                             |
| `npm run lint`   | ESLint（`eslint.config.ts`）   | 容易写错的代码：未使用变量、`v-for` 缺 `key`、直接改 props…… |
| `npm run format` | Prettier（`.prettierrc.json`） | 排版（无分号、单引号、100 列）                               |

**排版只由 prettier 决定**：所以 ESLint 用的是 Vue 的 `flat/essential` 规则集（只含会出错的
规则），不引入 `max-attributes-per-line` 那类排版规则，也就不需要 `eslint-config-prettier`。
改 lint 规则时请保持这个前提，否则两个工具会互相推翻对方的结果。

## 代码结构

```
src/
├── api/            axios 封装，按后端 scope 分文件
│   ├── http.ts       baseURL='/api' + token 注入 + 401 自动登出（唯一一处定义前缀和鉴权头）
│   ├── auth.ts       /auth/login、/auth/register、/me（读自己 / 改自己，任何已登录用户）
│   ├── users.ts      /admin/user/*、/admin/users（都要 ADMIN，含审批通过、改权限密码）
│   ├── taxonomy.ts   /taxonomy/*（公开读：阶元 / 分类树 / 名录 / 记录）
│   ├── staffTaxonomy.ts  /staff/taxonomy/*（STAFF 或 ADMIN 的增删改）
│   ├── maintenanceLogs.ts  /staff/maintenance-logs/*（读 + 写，STAFF 或 ADMIN）
│   └── birdRecords.ts  /bird-records/*（公开读）与 /staff/bird-records/*（写）
│   └── birdSurveys.ts  /bird-surveys/*（公开读）与 /staff/bird-surveys/*（写，含 GPX 导入）
├── stores/auth.ts  Pinia：token + 当前用户；isAdmin / isStaff 由 role 推导
├── stores/pendingUsers.ts  Pinia：待审批人数，供「用户管理」菜单的红点使用
├── router/         路由表 + 守卫（requiresAuth / requiresAdmin）
├── layouts/        AppLayout：顶栏 + 导航 + 登录态
├── components/     TaxonPicker（逐级下拉框）、TaxonTreeNode、SpeciesRecordTable、ConfirmDialog、
│                   ListPager（分页条：总页数 + 跳页）
├── views/          LoginView、ProfileView、UsersView、
│                   SpeciesListsView（名录）、TaxonomyTreeView（分类树）、
│                   BirdSurveyView（鸟类调查）、MaintenanceLogsView（维护日志）
├── taxonomy.ts     分类树 / 名录的类型与阶元辅助（阶元标签、深度、taxonLabel）
├── maintenanceLogs.ts  维护日志的类型
├── birds.ts        鸟类调查「重要记录」的类型
├── birdSurveys.ts  鸟类调查「鸟调记录」的类型
├── pagination.ts   分页响应的类型与页码辅助（Paged / pageCount / clampOffset）
├── types.ts        与后端用户 DTO 对应的类型
└── validation.ts   与 backend/src/*_models.rs 的长度常量对齐的前端校验
```

## 几个约定

- **列表接口都返回 `{ items, total }`**（后端 `taxonomy_handlers::Paged`），不是裸数组。
  `total` 是显示总页数、跳到指定页的前提（用 `components/ListPager.vue`），
  光靠「这一页有没有装满」只能猜下一屏还有没有。新增列表接口时按这个形状返回，
  并且**取总数和取当前页要共用同一组筛选条件**（见各 handler 里的 `*_query`）。
  删完当前页的记录后 `total` 会变小，页面用 `pagination.ts::clampOffset` 退回末页。
- **所有后端请求都带 `/api` 前缀**，由 `api/http.ts` 的 `baseURL` 统一加上，
  所以 `api/` 里的函数写的仍是后端真实路径（`/auth/login`、`/admin/users`），
  与 `backend/src/main.rs` 的 scope 一一对应。新加接口时也只写在 `baseURL` 之后。
- **`role` 是字符串，不是整数**：`""`（无权限）/ `"ADMIN"` / `"STAFF"` / `"ADMIN | STAFF"`。
  空权限的**返回值是空字符串**，所以下拉框里「无权限」的值是 `''`。
  `""` 同时意味着「自助注册后、还没被授予 STAFF」（`types.ts::isPending`）：
  这种账号能登录浏览、维护自己的资料，只是不能上传 / 修改物种数据。
  细节见 `types.ts`。
- **改密码会让当前 token 立即失效**（后端把 `token_version` +1），
  `ProfileView` 因此在成功后会主动登出并回到登录页，带上 `?notice=password-changed`。
- **图片、文案里的错误信息来自后端**：`{ "error": "..." }` 原样展示（见 `api/http.ts::errorMessage`）。
- 长度限制（名字/昵称 10 字符、密码 8 字符 / 72 字节，注册信息里的真实姓名 50、
  学号 50、单位 200、联系方式 200、微信 100）在 `validation.ts` 里，
  和 `backend/src/models.rs` 的常量手工对齐 —— **改后端时两边都要改**。
  分类树 / 名录的同类常量（学名 200、名录名 100、编号 50）也在那里，对齐
  `backend/src/taxonomy_models.rs`。

## 页面与权限

| 路由           | 谁能进        | 做什么                                                                                |
| -------------- | ------------- | ------------------------------------------------------------------------------------- |
| `/login`       | 所有人        | 登录、自助注册（注册后为无权限账号，可正常使用，等管理员授予写权限）                  |
| `/`            | 所有人        | 「物种名录」：选名录，按关键词 / 类群浏览记录（读接口公开）                           |
| `/tree`        | 所有人        | 「分类树」：展开分类树、查看节点路径与子节点                                          |
| `/birds`       | 所有人        | 「鸟类调查」：鸟种清单 / 重要记录 / 鸟调记录                                          |
| `/logs`        | STAFF / ADMIN | 「维护日志」：按名录浏览 / 搜索修订记录（连读都要权限）                               |
| `/profile`     | 已登录        | 「个人中心」：改自己的注册信息（昵称/真实姓名/学号/单位/联系方式/微信）、改自己的密码 |
| `/admin/users` | ADMIN         | 待审批用户通过、用户增删查、改权限/密码、查看注册信息                                 |

前端的路由守卫只是**界面层的便利**，真正的权限判定始终在服务端：
`/admin/*` 由 `PermissionGuard::all(ADMIN)` 拦住，`/staff/taxonomy/*` 与
`/staff/maintenance-logs/*` 由 `PermissionGuard::any(STAFF | ADMIN)` 拦住，
权限不足时拿到的是 403。

## 物种分类前台

物种浏览是**公开**的（后端 `/taxonomy` 读接口就不需要登录），只有维护动作要
STAFF 或 ADMIN；路由见上面的「页面与权限」。

浏览记录只在一个地方：「物种名录」页先选一份名录，再按**关键词 / 类群**筛这份名录里的
记录。类群筛选就是 `TaxonPicker` + 「包含下级类群」两个字段（对应 `/taxonomy/records` 的
`taxon_id` / `descendants`）。原来独立的「物种检索」页已去掉 —— 它只比这里多一个跨名录
查询，确认用不上；老链接 `/search?...` 会原样转给名录页。

**选了类群，名录跟着类群走**：按「搜索」（或带 `?taxon=` 进页）时，如果当前这份名录里
没有该类群的记录，就换成「有该类群记录」的第一份（按 `position`）。所以在高等植物名录里
选鸟纲会跳到脊椎动物名录，而不是对着空列表。候选集合由后端算：
`GET /taxonomy/lists?taxon_id=&descendants=` 只返回有该类群记录的名录（按记录表 EXISTS
子查询），不把名录范围写死在前端，新增名录 / 补录记录都自动跟上。当前这份里有该类群就
不动；不选类群时完全按原样在「这份名录」里搜。

两个实现上的重点：

- **逐级下拉框**（`components/TaxonPicker.vue`）不是「上一级的直接子节点」，
  而是「最近一个已选祖先的直接子节点」：`GET /taxonomy/taxa?rank=<本级>&parent_id=<祖先>`。
  因为后端允许跳级（属直接挂在纲下），某一级下拉框可能是空的，但更低的几级
  仍能列出跳过中间阶元的节点。`modelValue` 取选中的**最深**节点。
- **三态字段**：`parent_id` / `chinese_name` / 记录的 `note` 等，请求体里
  字段缺失 = 不改，`null` = 置空，有值 = 改。见 `api/staffTaxonomy.ts` 的注释。
  例外的陷阱是**名录的 `description`**：后端用的是普通 `Option`，`null` 也只
  表示「不改」，所以「清空描述」只能写空字符串。

界面权限（有没有「新增 / 编辑 / 删除」按钮）由 `stores/auth.ts` 的 `isStaff`
决定，真正的权限判定始终在服务端（`/staff/taxonomy/*`、`/staff/maintenance-logs/*`
由 `PermissionGuard` 拦住）。

维护日志（`MaintenanceLogsView`）是唯一一个**连读都要 STAFF / ADMIN** 的资源：
游客和无权限用户既看不到菜单入口，直接访问 `/logs` 或 `/staff/maintenance-logs`
也会被挡回。「修订说明 / 物种附录」用 `white-space: pre-wrap` 原样保留换行。

## 鸟类调查

「鸟类调查」（`BirdSurveyView`）是三个标签页，三种数据来源：

- **鸟种清单**：鸟纲下的全部名录记录，走的是现成的 `/taxonomy/records`。
  鸟纲节点 id 不写死 —— 导入脚本按 UUIDv5 派生 id，换一份数据就不一样了，
  所以先用 `findTaxonId('class', 'Aves')` 在分类树里找到节点，再用
  `descendants` 取整棵子树。
- **重要记录**：`bird_records` 表（迁移 `2026-10-11-000000`），字段是
  记录人 / 时间 / 地点 / 备注 + `taxon_id`（关联分类树）+ 来源。
  `observed_at` 是**原文**而不是日期：「2023年5月底」这类写法归一成一个日期
  会丢精度。时间范围筛选靠迁移 `2026-10-11-010000` 加的两个**生成列**
  （`observed_from` / `observed_to`）：原文归一成一段日期区间，查询时求交集，
  「2023年5月底」落在 5 月下旬、「2007年」整年都算。归一逻辑只在数据库函数
  `bird_observed_bounds` 里一份，导入 / 新建 / 修改共用。
  读接口公开，写接口在 `/staff/bird-records`，所以页面上的
  「新增 / 编辑 / 删除」由 `isStaff` 控制。新增 / 编辑的物种**只能选鸟纲**：
  不再逐级选门 / 纲 / 目 / 科，而是一个带筛选词的鸟种下拉框（候选和「鸟种清单」
  同一个来源）；学名与中文名随鸟种自动带出，不手填。
- **鸟调记录**：`bird_surveys` / `bird_survey_records` 表（迁移
  `2026-10-14-000000`）。一次鸟调是表头（日期 / 领队 / 天气 / 备注 / 来源），
  明细是逐条的鸟种观察（数量 / 时间 / 经纬度 / 备注）。读接口公开，写接口在
  `/staff/bird-surveys`（含 `POST /staff/bird-surveys/import`）。
  - **「鸟种数」不落库**：读接口按明细里的不同鸟种现算（关联了分类的按
    `taxon_id` 去重，未匹配的按中文名去重），明细一改就跟着变。
  - **GPX 导入**（`backend/src/gpx.rs`）：一个 `<wpt>` 一条明细，`<name>` 是
    鸟种中文名、`<desc>` 是数量、`<time>` 是 UTC（入库换算成北京时间）、
    `gpxx:StreetAddress` 是备注。中文名能对上鸟纲（Aves）节点的关联 `taxon_id`，
    对不上的（旧名 / 名录里没有）保留原名、`taxon_id` 为空，界面标「未匹配」。
    导入时只填领队，日期 / 天气会从文件名（`20180522北大鸟调多云转晴.gpx`）
    猜出来作为默认值，可以改；原始 GPX **不落盘**，只把文件名记进 `source`。
  - 页面上的「导入 / 新增 / 编辑 / 删除」由 `isStaff` + 管理模式控制。
