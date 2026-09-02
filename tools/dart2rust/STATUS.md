# dart2rust 进度

目标:把 `/d/linzjUbuntu2204/gallery_upstream`(真正的 flutter/gallery)翻译成
Rust 跑起来。这需要翻译 gallery 自身的 Dart **和**它依赖的整个 Flutter framework。

尺子是 `bin/census.dart`:对一棵树跑前端,把拒绝原因**按类别归并**后排队。
队头就是下一件该做的事。用法:

    dart run --packages="E:/source/flutter/.dart_tool/package_config.json" \
        tools/dart2rust/bin/census.dart <目录> [--examples]

---

## 第 2 轮:具名参数

`painting/` 普查队头是 **具名参数 103 次**,远超第二名。

### 为什么这件事有静默出错的余地

Rust 没有具名参数,所以 Dart 的具名调用必须压平成位置调用。
**按调用点顺序压平**在绝大多数调用上都是对的——因为大多数人本来就按声明顺序写。
但 Dart 允许 `Rect.fromLTRB(top: a, left: b)`,这时两个参数会**静默互换**。

所以顺序必须来自**被调方的形参表**。先验证这一点可达:
`edge_insets.dart` 的 **130 个调用点,130 个都拿到了被调方的形参表**。

省略的可选参数也必须补上值,因为生成的 Rust 函数每个参数都是位置的:

- 有字面量默认值 → 按源码文本降下来
- 省略且可空 → `None`,这正是 Dart 给它的值
- 其余 → **停下**。编译器猜出来的默认值和上游写的默认值长得一模一样,
  这是最难在后期发现的一类错。

### 测试要能分辨"对"和"看着对"

fixture `testdata/fixtures/named_args.dart` 的调用**故意不按声明顺序写**,
权重取 10 的幂,好让任何错误排列都落不到正确的和上。

这个 fixture 当场抓出一个 bug:`allDefaults()`(一个具名参数都没给)
走了 `named.isEmpty` 的捷径,**三个默认值一个都没填**,生成了 `weigh()`。
捷径本身是错的——被调方已知时,每个参数都要交代。

变异验证,两个都被杀死:

| 变异 | 结果 |
|---|---|
| 具名参数按调用点顺序取 | 2 个测试失败 |
| 恢复 `named.isEmpty` 捷径 | 生成的 Rust 元数不匹配,rustc 拒绝 |

第二个是 rustc 报出来的,不是我的断言——记准确:它是真杀,但杀它的是编译器。

### 结果

`painting/` 普查:**具名参数 103 → 0**,翻译出的成员 268 → 329。
测试 9 个全过。

---

## 第 3 轮:队头是虚的,然后 `assert`

### 先查上一轮记的疑问,结果是记账 bug

`_lowerMethod` 里私有检查排在**最后**:

    if (member.isAbstract) throw ...;   // 先
    if (member.isSetter) throw ...;
    if (name.startsWith('_')) return;   // 后

所以 `double get _x;`(私有**且**抽象)被记成 `abstract method`,
`set _foo(v)` 被记成 `setter`。私有成员应该是**跳过**,不是拒绝——
库外没人能叫它的名字。字段那边是同一个形状:私有 `static final` 被记成
`non-const static field`。

改成私有优先后:**`abstract method` 74 → 57**,17 个是假的。

**队头有一部分是虚构的,会把下一轮送去建错东西。** 这就是为什么要先查。

### 真队头:`assert`(65)

Dart 的 `assert` 和 Rust 的 `debug_assert!` 是同一件事——debug 生效、release 编译掉。
这种对应精确到可以**翻译**而不是模拟。用 `assert!` 会把上游每一条检查
都留在 release 二进制里,那是这个编译器没资格替人做的性能决定。

三个判断:

- **消息是诊断,不是契约。** assert 的含义是它的条件,firing 与否和消息无关。
  所以纯字符串消息带过去,插值/调用的消息**保留源码在注释里**而不翻译——
  翻译它会把字符串格式化连同消息里调用的一切,拖进整棵树里每条 assert 的依赖集,
  只为了改变"出错之后"debug 构建打印什么。
- **带 assert 的构造函数不能再是 `const fn`。** 保 `const` 就得丢检查,
  这里选择保检查。
- 转义:反斜杠必须在双引号**之前**加倍,否则刚加的那个反斜杠会被下一步再加倍。

### 一个 bug 和它的两个同类

生成的消息一开始每个字符前面都多了反斜杠——`replaceAll(r'', ...)` 是**空模式**,
Dart 会在每个字符之间插入替换串。修的时候发现**字符串字面量那里是同一个 bug**
(根本没转义),一并修了。写这份记录时又栽了第三次(Python 的 `\u`)。
**转义要写在能看见它的地方,不要写在 heredoc 里穿三层。**

### 测试:每条都要有能触发它的搭档

否则"assert 翻译过来了"和"assert 被静默丢掉了"是同一个观察——
所有不触碰条件的测试两种情况下都通过。

变异三个全杀:

| 变异 | 结果 |
|---|---|
| assert 求值但不检查 | 3 个测试失败 |
| 字面量消息被丢掉 | 2 个失败 |
| 构造函数的 assert 不生成 | 1 个失败 |

### 结果

`assert` 语句 65 → 0,初始化列表 12 → 0,成员 296 → 305,测试 13 个全过。

**几项计数上升了**(后缀 41→46、super 40→43、级联 13→18、插值 35→38):
原先那些成员在 assert 处就被拒了,现在 assert 过去了,同一成员里的
**下一个**障碍才浮现。这是真进展,不是退步。

---

## 第 4 轮:命名构造函数,以及第一次拿真上游代码当测试

Dart 的命名构造函数和 Rust 的关联函数是**同一个形状**——
`EdgeInsets.all(8)` 和 `EdgeInsets::all(8.0)` 是同一个调用,
无名的那个按 Rust 惯例叫 `new`。什么都不用编码,所以什么都没编码。

先查了上游怎么写的:`EdgeInsets` 的四个命名构造函数全是**直接字段初始化**,
没有重定向。第 3 轮做的 assert 和字段初始化列表正好直接用上。

拒绝的三种,各有各的理由:

- **factory** —— 可能返回缓存实例或子类,不是"构造 Self 的关联函数",要类层次
- **重定向**(`: this.fromLTRB(...)`)—— 可以降成对另一个构造函数的调用,
  但那个构造函数的字段初始化还够不着,所以停下,而不是生成一个**不设任何字段**的构造函数
- **super 构造函数调用** —— 同样要类层次

### 拿真的 EdgeInsets 当测试,而不是合成 fixture

代价是要写 `Radius`/`RRect`/`ViewPadding` 的桩(约 30 行)。
回报是**当场抓到一个合成 fixture 抓不到的编译器 bug**。

`copyWith` 里的 `left ?? this.left` 生成了 `left.unwrap_or(*self.left)`——
`*self.left` 会解析成 `*(self.left)`,解引用的是字段不是接收者。

根因:**`this` 在 Rust 里按位置有两个身份**。
当**值**用时是 `*self`(结构体的一份拷贝),这是 `return this;` 要的;
当字段或调用的**接收者**用时必须是 `self`。现在由 `_receiver()` 统一决定。

桩这边也暴露了一件事,记下但不在本轮做:
上游 `Rect` 的 `right`/`bottom`/`width`/`height` 是 **getter**,
而译出来的代码按**字段**读。桩里先做成字段对付过去。
**Dart getter → Rust 方法**是一件真事,该有自己的一轮。

### 变异:四个全杀,但要分清是谁杀的

| 变异 | 结果 | 杀手 |
|---|---|---|
| 所有构造函数都生成成 `new` | DOES NOT BUILD | rustc |
| 只生成第一个构造函数 | DOES NOT BUILD | rustc |
| `this` 当接收者时退回 `*self` | DOES NOT BUILD | rustc |
| **字段初始化式整体轮转一位** | **4 个测试失败** | **断言** |

前三个都是 rustc 抓的,那说明我的断言在这三处没出力。
所以补了第四个:它**类型完全正确**(全是 f32),编译得过,
只是每个字段拿了下一个字段的初始化式。它挂了 4 个测试——
断言确实在检查**值**,不只是在检查"能编译"。

### 结果

`EdgeInsets` 拒绝 **13 → 5**,`painting/` 命名构造函数 62 → 0,测试 **20 个全过**,
其中 7 个跑的是**真的上游 EdgeInsets**。

---

## 第 5 轮:类层次第一刀——抽象类变 trait

上一轮末尾量出来:`abstract`(778)+ `super`(692)+ super 构造(212)+ `is`(182)
+ factory(115)= 全 framework 26% 的拒绝,**而且是同一件事**。本轮动它。

### 前提:编译器必须一次看整个文件

要生成 `impl AlignmentGeometry for Alignment`,得知道
`AlignmentGeometry` 是抽象的、它要求哪些方法——
这两件事**从 `Alignment` 内部都看不见**。
所以加了 `IrLibrary` 和 `lowerLibrary`,驱动多了 `--all` 模式。

### 映射:Dart 已经做好的区分,Rust 用同样的方式表达

- 抽象类 → **trait**(Rust 里"一组操作、自己不持有存储"就是这个)
- 抽象成员 → trait 的**必需方法**
- 有实现的成员 → trait 的**默认方法**

Dart 自己就是按"有没有函数体"分的,Rust 也是。**什么都不用发明。**

带不过来的是**字段**:Dart 的抽象类可以声明存储,Rust 的 trait 不行。
这种字段**在输出里报出来**,不是悄悄丢掉。

抽象类当**类型**用时是 `dyn Trait`——不是风格问题:
`fn add(other: AlignmentGeometry)` 根本编译不了,
Rust 不知道 `AlignmentGeometry` 有多大。所以拥有时 `Box<dyn>`、借用时 `&dyn`。

### 协变返回:两种语言真正不同的地方

Dart 允许 `Alignment operator -()` 覆盖返回 `AlignmentGeometry` 的方法。
Rust 要求 impl **精确**返回 trait 声明的类型。

解法是**委托**:`Alignment` 保留它自己返回 `Alignment` 的 `impl Neg`
(这是 Rust 调用者想要的),trait 方法调它再装箱。
一个函数体,两个返回类型,不用把 body 生成两遍。

### 跨轮次冲突,以及我第 3 轮判断错了

真代码撞出来的:`TextAlignVertical` 构造函数里有 assert,
又有由它构造的 `static const` 字段。第 3 轮我说
"带 assert 的构造函数不能是 `const fn`,保 const 就得丢检查"——

**这句话是错的。** const panic 从 Rust 1.57 起就稳定了,
`const fn` 里的 `debug_assert!` 编译得过,而且运行时照样触发。
我单独写了个最小例子验的。**两个从头到尾都能保住**,那次取舍根本不必要。
这一轮改回来了,并且这个错只在两轮的规则在真代码上相遇时才暴露。

### 变异:五个,四杀一等价

| 变异 | 结果 |
|---|---|
| 抽象类当成 struct 生成 | DOES NOT BUILD |
| 不生成 base 的 impl | DOES NOT BUILD |
| `_matching` 永远返回 null(全 `todo!()`) | 3 个测试失败 |
| 协变返回不装箱 | DOES NOT BUILD |
| 委托写成 `self.method(...)` | **存活** |

最后一个第一次跑时**我的变异自己没编译**(Python 里多写了 `\$`),
按老规矩重写后再跑——**不编译的变异不算数**。

重写后它**存活**:`self.method(...)` 和 `Type::method(self, ...)` 行为完全一样,
因为 Rust 优先解析固有方法。这是**等价变异**,如实记下。

显式路径保留,理由是它防的是固有方法**缺席**时解析到 trait 方法自己、
从而无限递归。但当前语料里构造不出这个情形
(`_matching` 找得到的方法,固有版本一定也生成了),
所以**这条防护测不出来**,不是测试杀死了它。

### 结果

`painting/`:`abstract method` 57 → 0,零拒绝的类 2 → 6。
**全 framework**:拒绝 7798 → **7012**,零拒绝的类 347 → **438**。
测试 24 个全过,其中 4 个走 `dyn AlignmentGeometry` 动态分发。

---

## 第 6 轮:`super`,以及一个必须更正的记录

### 更正:`package:kernel` 一直都在

README 里"Kernel 取不到"那段理由**是错的**,现在已改。它在:

    E:/source/flutter/engine/src/flutter/third_party/dart/pkg/kernel/

我上次搜的是 `engine/src/third_party/`,而它在 `engine/src/**flutter**/third_party/dart`——
从 `E:/source/flutter` 算是第 7 层,我用了 `-maxdepth 6`,**正好差一层**,
然后据此写下了"取不到"。是用户指出 upstream 源码在 `engine/src` 才发现的。

验证过它能用:拿 `pkg/kernel` 读 SDK 缓存里的 `dart2js_platform.dill`,
报 `Unexpected Kernel Format Version 140 (expected 139)`——
**说明它读到了文件、解析了头,只是版本差一个**。
改读同一 checkout 里修订版匹配的
`engine/src/out/host_release/flutter_patched_sdk/platform_strong.dill`:

    libraries: 20   classes: 1374   procedures: 748
    sample: AsyncError (abstract=false, super=Object), ...

**Kernel 管线是通的**,而且同一 checkout 里有修订版匹配的
`frontend_server_aot.dart.snapshot`,能产出配套的 app.dill。

用户还指出了更重要的一点:**要发布只能走工具链构建好的 app.dill**。
这是对的——app.dill 是**整个程序**,链接完毕、可达性确定、常量已求值、
mixin 已展开,而按文件翻译源码永远只是在逼近它。

### 本轮做的仍然是 `super`(692),而且它换前端也不作废

Rust **没有 `super`**。一旦 impl 覆盖了 trait 的默认方法,默认实现就不可达了——
`Trait::name(self)` 会派发回覆盖版。而"在覆盖 `X` 的方法里调 `super.X()`"
**不是边缘情况**:painting/ 和 rendering/ 里 435 个 super 调用**全是**这个形状。

所以每个抽象类的有实现方法生成**两份**:
一个自由泛型函数持有函数体,trait 的默认方法调它。
`super.X(..)` 就指那个函数——**唯一一个不可能派发到别处的东西**。

Kernel 前端下这件事只会更容易(Kernel 的 super 调用已经解析到具体目标),
Rust 这一侧的问题一模一样,所以这一轮的工作 100% 转移。

### 两个粒度 bug,同一个教训

1. **后端按类拒绝,前端按成员拒绝。** super 通了以后,`Alignment.add`
   不再因 super 被拒,改为因旁边的 `is` 被拒——**整个 `Alignment` 类因此消失**。
   现在后端也按成员拒绝,拒绝的那个成员在输出里留 `// NOT TRANSLATED` 和原因。
2. 更早一步:`emitLibrary` 一个类失败就丢掉整个文件。也改成按类。

**拒绝的粒度应该等于工作的粒度。** 两次都是这条。

### 还有一个只有 `dyn` 才能发现的 bug

`_emitBaseImpl` 原来只为**抽象**方法生成 override。
子类覆盖基类的**有实现**方法时,impl 里没有这一项,
于是走 `dyn Base` 会调到 trait 默认实现——**固有方法仍然是对的**,
所以只有穿过 `dyn` 的调用能看出来。测试因此全部走 `dyn`。

### 变异:四个,三杀一等价

| 变异 | 结果 |
|---|---|
| `super` 降成 `self.method(..)` | **栈溢出**,两个测试崩溃 |
| 覆盖基类有实现方法时不进 impl | 2 个测试失败 |
| 对未翻译的基类方法照样生成 super 调用 | DOES NOT BUILD |
| trait 默认方法内联 body 而不委托 | **存活** |

第一个我的脚本报的是 `NO RESULT`,单独跑确认了是
`thread ... has overflowed its stack`——**是杀死,不是无结果**,查清楚才记。

第三个第一次跑时**存活**,因为语料里没有这种情况。补了 fixture
(基类方法用了未支持的级联)之后它才被杀。**测不到的防护等于没有防护。**

第四个是**真等价**:自由函数照样生成,`super` 走的是它,
内联与否只是重复一份函数体,不改行为。

### 结果

测试 27 个全过,其中 3 个穿 `dyn Shape` 验 super。

---

## 第 7 轮:gallery 的 app.dill 造出来了,而且读得动

上一轮定的:**先造 dill 并确认能读出 gallery 自己的类,再动前端**——
造不出来的话,整条路走不通,要在写代码之前就知道。

### 结果:通了

    libraries: 1317   classes: 7467

| 来源 | 类数 |
|---|---|
| `package:flutter` | 4281 |
| `dart:*` | 1374 |
| **`package:gallery`** | **689** |
| `package:flutter_localizations` | 363 |
| `package:get` | 208 |

105 MB,frontend_server 零错误退出。

### Kernel 已经替我们做完的事(analyzer 前端要自己做的)

| | |
|---|---|
| 已展开的匿名 mixin 应用 | **871** |
| gallery 里的 super 调用,**目标已解析** | **285**(`initState -> State.initState`) |
| gallery 里已求值的 const 字段 | **273** |

第 6 轮花了一整轮解决 super 调用要指向谁的问题;Kernel 里它**本来就带着目标**。
871 个 mixin 应用在 analyzer 前端下是 871 次要自己做的类层次推导。

### 修订版匹配是这件事的全部难点

Flutter SDK 的 `bin/cache/dart-sdk` 和引擎里的 Dart checkout **是不同修订版**,
而 Kernel 二进制格式带版本号。用一边的 `pkg/kernel` 读另一边产出的 dill:

    Unexpected Kernel Format Version 140 (expected 139)

引擎 checkout 里恰好有**完整的一套同修订版工具**——
它自己编出来的 `dart-sdk`、`dartaotruntime`、`frontend_server` 快照、
patched platform、`pkg/kernel`,全在 `bb17f25f...`。
所以工具里的每一条路径都取自那一棵树,**一条都不来自 Flutter SDK**。

这就是 `bin/dill.py` 存在的理由,而不是把命令写进 README:
路径的修订版一致性是个静默失败的陷阱,
`--check` 会把八个位置和 revision 一次列清楚。

### 工具

    python tools/dart2rust/bin/dill.py --check
    python tools/dart2rust/bin/dill.py --config <out.json> [--config-root <dir>]
    python tools/dart2rust/bin/dill.py --build package:gallery/main.dart \
        --packages <app>/.dart_tool/package_config.json -o app.dill

    dart run --packages=<out.json> tools/dart2rust/bin/dill_info.dart app.dill

`--build` 是**用提交进仓库的工具重跑过一遍**的(105.1 MB,exit 0),
不是把 scratchpad 里手敲的命令抄进文件——没跑过的工具不算验证。

一个细节记下:frontend_server **编译出错也可能 exit 0**,
所以 `build()` 另外扫 stdout 里的 `Error:` 行,不只看退出码。

---

## 下一步:写 Kernel 前端

前置条件已全部验证,可以动手了。

1. `lib/frontend_kernel.dart`:Kernel `Component` → 现有 `IrLibrary`
2. `lib/ir.dart`、`lib/backend_rust.dart`、`testdata` 的 27 个测试 **一律不动**
3. **验收标准**:同一个 `alignment.dart`,Kernel 前端产出的 Rust
   要能让那 27 个测试全过。**同样的 IR 应当产出同样的 Rust**——
   这是一个前端能不能替换掉另一个的唯一诚实检验。

先做**一个类**跑通(建议 `Alignment`,已有 7 个测试盯着它),
再铺开。别一上来写全量遍历。

要当心的两处,现在就记下:

- **Kernel 是脱糖过的**。`a + b` 是 `InstanceInvocation`,不是 `BinaryExpression`;
  隐式转换是显式节点;`for-in` 已经展开成迭代器循环。
  产出的 Rust 会比 analyzer 版**难看**,这是这条路的真实代价。
- **Kernel 里没有私有/公开的区别**那么简单:`_x` 就是名字里带 `_` 的成员,
  但它带 `Library` 归属。现在前端"跳过私有成员"的规则要重新想——
  在整程序视角下,私有成员是**必须翻译**的(gallery 的实现全在私有 State 类里),
  不能再跳过。**这会让拒绝数大涨,那是对的**,现在的低拒绝数有一部分是靠跳过换来的。

## 参考:analyzer 前端的队头(全 framework 7012 次拒绝)

| 次数 | 要建的东西 |
|---|---|
| 499 | setter |
| 367 | 后缀 `!` |
| 285 | 赋值表达式 |
| 212 | super 构造函数调用 |
| 209 | 调用函数值(闭包) |
| 197 | 字符串插值 |
| 181 | `is` |

换前端后要重新普查,这些是 analyzer 视角的数。

**"零拒绝"这个数系统性偏乐观**,别用它报进度。上次全量:
347 零拒绝 → 254 能解析 → 235 不含未翻译 Dart 类型 → **验证过能编译的只有 2 个**。
