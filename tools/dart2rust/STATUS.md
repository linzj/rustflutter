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

## 第 8 轮:Kernel 前端,两个前端产出一致

验收标准是上一轮定死的:**`ir.dart`、`backend_rust.dart`、27 个测试一行不改**,
Kernel 前端产出的 Rust 要让它们全过。

**达到了。**

    analyzer  ok. 27 passed
    kernel    ok. 27 passed

    212 行代码 vs 212 行,22 行不同

而且那 22 行**全是同一个外观差异**:analyzer 产 `Alignment::new((-1.0), (-1.0))`
(它看到的是 `IrUnary('-', 1.0)`),Kernel 产 `Alignment::new(-1.0, -1.0)`
(常量求值直接给出负数)。两个都对,Kernel 的更干净。

### 先 dump 节点,再写代码

Kernel 是脱糖过的,照 Dart 源码猜它的形状正是要避免的错误。
所以先写了个探针把 `Alignment` 里**真正出现**的节点种类全印出来,照着写:

- 算术**全是方法调用**:`x - other.x` 是 `InstanceInvocation(x, '-', [other.x])`,
  `this.{Alignment.+}(other)` 也是。要**映射回二元表达式**,
  否则会生成 `a.add(b)`——既不是上游写的,也不是后端运算符 trait 期待的。
  这一步是两个前端 IR 能对上的关键。
- 运算符方法是 `Procedure(kind: Operator)`,名字就是 `+`、`unary-`、`~/`
- 字段读是 `InstanceGet`,`this.{Alignment.x}`
- 局部变量声明是 `VariableStatement` 包着 `VariableDeclaration`
- 名字访问器是 `cosmeticName`(人写的名字;CFE 造的临时变量没有,
  或者以 `#` 开头)

### Kernel 白给的东西

- **super 调用带着已解析的目标**(`node.interfaceTarget.enclosingClass`)——
  第 6 轮整轮在解决的问题,这里是现成的
- **常量已求值**:`const Alignment(-1, -1)` 到手就是类加字段值。
  重建成构造函数调用而不是结构体字面量,好让产出的 Rust 仍然读作
  `Alignment::new(-1.0, -1.0)`——值一样,而源码形态还认得出
- **默认参数值是表达式**,不像 analyzer 那边只能读源码文本再认字面量
- 匿名 mixin 应用会被跳过(`isAnonymousMixin`),父类往上找到第一个真类

### diff 抓到的一个真 bug

`TextAlignVertical::new()`——**少了参数**。它的构造函数是
`const TextAlignVertical({required this.y})`,而我的常量处理只走了
`positionalParameters`。位置参数**和**具名参数都要走,顺序和
`_lowerConstructor` 一致。

**这个 bug 是 diff 发现的,不是测试**:两个前端的输出摆在一起才看得出来。

### 真实损失:Kernel 里没有文档注释

analyzer 版 492 行,Kernel 版 299 行,差的几乎全是 doc comment——
dill 里不带它们。这是这条路要付的代价,记在这里而不是发现时再惊讶。

### `agree.py`:把"两个前端一致"变成可重复的检查

    python tools/dart2rust/bin/agree.py --dill <app.dill>

它两个前端各生成一次、各跑一遍完整测试、再 diff 代码行(去掉注释,
否则会被 analyzer 那一侧的 doc 淹没),最后把工作树还原。

**做过一次的事和能重复的检查不是一回事**——这一条以后每轮都能跑。

---

## 第 9 轮:翻译私有成员,拒绝数涨了 70%

这一轮作废一条规则:**两个前端都在跳过私有成员**。
按文件翻译时那是对的——库外没人能叫它的名字。
整程序视角下**是错的**,而且错在要害:Flutter 每个 StatefulWidget 的实现
都在私有 State 类里,gallery 自己的 689 个类同理。

预测过会大涨,涨了:

|  | 之前 | 之后 |
|---|---|---|
| 类 | 1931 | **3331** |
| 拒绝 | 7012 | **11869** |
| 已生成成员 | 15313 | **21095** |

**这不是退步,是尺子第一次量到真东西。** 之前那个低拒绝数有一部分是
靠"看得少"换来的——`CupertinoApp` 报零拒绝,因为它的行为全在
`_CupertinoAppState` 里,而普查连那个类都没看。普查自己也有一条
私有类过滤,一并去掉了。

### Rust 侧怎么表达 Dart 的私有

Dart 的私有是**按库**的,Rust 的是**按模块**的,而一个文件的类正好
生成到一个模块里——所以 `_` 前缀的成员不加 `pub` 就得到同样的可达性。
**名字不动**,`_x` 本来就是合法的 Rust 标识符,改名会让输出没法和上游对照搜。

### 私有成员一进来,四个真 bug 就掉出来了

1. **Dart getter 不是 Rust 字段。** `AlignmentGeometry._x` 是抽象 getter,
   变成 trait 方法,当字段读就是
   "attempted to take value of method `_x`"。这笔账第 4 轮就记下了,现在必须还。
   - Kernel 直接说了是 `Field` 还是 `Procedure`;
   - **analyzer 有个陷阱**:它把**字段**读也解析成 `PropertyAccessorElement`
     (字段的隐式访问器)。所以"是不是 PropertyAccessorElement"问错了,
     要问的是 **`isSynthetic`**——隐式的是合成的,真 getter 不是。
2. **`operator ==` 生成了 `op_61_61`**,`superFn` 生成了
   `alignment_geometry_super_`(空名字)。运算符名字必须是合法标识符,
   而且要可读:现在每个 Dart 运算符都有名字,认不出的**停下**而不是写成十进制。
3. **`Alignment._stringify(...)` 被调用但从未生成**——第 1 轮那个 bug
   换形态回来了。当时我用"拒绝一切私有引用"挡住它,那条钝规则一撤,它就回来了。
   正确的规则和 `_superCall` 同形:**被调方在本文件里,就必须在 IR 里**。
4. **具体类型返回到抽象返回位置要装箱**。
   `AlignmentGeometry.add` 结尾是 `_MixedAlignment(...)`,
   声明返回 `AlignmentGeometry` = `Box<dyn AlignmentGeometry>`。
   这是第 5 轮协变返回问题在**函数体里**的版本。
   只对 `IrNew` 装箱——只有构造函数调用**确知**产出那个具体类型,
   别的可能已经是 box,多包一层会静默出错。

另外 analyzer 前端把顶层函数 `clampDouble(a,b,c)` 当成了 `self.clamp_double(...)`。
Kernel 前端一直是拒绝顶层调用的,**两边不一致,而且 analyzer 那边是错的**。

### 粒度教训的第三次

trait 那条路当时没跟上"按成员拒绝"的改动:一个 `toString` 里的字符串拼接
就把整个 `AlignmentGeometry` trait 带走,它的每个 impl 都编译不了。
现在 trait 的必需方法、默认方法、super 自由函数**全部按成员保护**。

配套一条:自由函数没生成时,trait 的对应默认方法不能去调它,
改成 `todo!()`——trait 和它的每个 impl 仍然对得上,少一个方法就对不上了。

### 桩反而变忠实了

`Offset.dx`、`Size.width`、`Rect.width/height`、`RRect.tlRadius`
上游**本来就是 getter**,桩里一直当字段。编译器分得清之后,桩改成方法,
**比之前更接近上游**。而 `Rect.left/top/right/bottom` 在 dart:ui 里
**确实是字段**——我一度把它们也改成 getter,那是改过头,查了 dart:ui 才改回来。

### 结果

    analyzer  ok. 27 passed
    kernel    ok. 27 passed
    AGREE

339 / 345 行,差 28 行。两个前端仍然一致。

---

## 第 10 轮:先把"赋值"这一条拆开,再做不需要可变性模型的那一半

上一轮留的话是"这个数字本身要先被信任"。查了,它确实是几件事。

Kernel 把赋值分成不同节点类型,所以拆分只是个计数:

| `package:flutter`(664 库,20922 次赋值) | |
|---|---|
| `VariableSet`(局部变量) | **10633(51%)** |
| `InstanceSet`(字段或 setter) | 10089(48%) |
| `StaticSet` | 188 |
| `SuperPropertySet` | 12 |

字段写里 6220 次穿过 `this`,3869 次穿过别的对象。
gallery 那边同样对半(700 / 667)。

**一半只需要 `let mut`,另一半才需要 `&mut self` 和"可变性怎么传染"的故事。**
照合并后的数字去规划,等于给两倍于实际需要的地方做难的那套。

本轮只做局部赋值。

### `let mut` 是关于整个函数体的事实

Rust 要在**声明处**写 `mut`,而需不需要是**整个函数体**的性质,不是那一行的。
所以生成前先走一遍函数体,收集被赋值的局部名。

参数也一样:Dart 的参数就是普通变量、随便重新赋值,
Rust 的参数默认不可变,`mut start: f32` 是说这件事的地方。
没有它,`shadow(start) { start = start + 1; }` 会生成一个赋不了值的赋值。

顺带:`_reassigned` 必须在**写签名之前**算好——签名先出来,参数的 `mut` 在签名里。

### 两个前端要在复合赋值上会合

analyzer 把 `x += 1` 保留成一个节点,Kernel 已经改写成 `x = x + 1`。
两边必须到达同样的 IR,所以 analyzer 那侧在前端就展开。

fixture 里 `total += step; total -= 1.0; total *= 2.0` 的**顺序**是故意的:
减法夹在中间,这样"`x *= 2` 展开成 `x = 2 * x`"这种错法也会被抓到——
只有加法和乘法的话,交换律会让错的展开也得出对的数。

### 一个变异存活,于是把它变成可检验的

**"所有局部都标 mut"存活了**——它也能编译、所有测试也都过,
差别只是一堆警告。这一点 fixture 自己的注释里就预言过。

所以给测试 crate 加了 `#![deny(unused_mut)]`:
**把"只给真正被重新赋值的局部标 mut"从一句风格主张,变成构建会检查的事。**
加了之后同一个变异 DOES NOT BUILD。

另一个变异("被重新赋值的参数不标 mut")第一次跑时**我写的 Dart 不合法**,
按老规矩重写——不编译的变异不算数。重写后也被杀死。

四个变异,全部杀死。

### 结果

    analyzer  ok. 31 passed
    kernel    ok. 31 passed
    AGREE

普查:`AssignmentExpressionImpl` 那一条消失了,
剩下 **1056 次"assignment to a field"**——这是它本来就该被分成的样子。

---

## 第 11 轮:后缀 `!`

Dart 的 `b!` 说"我保证它不是 null,不是就崩",Rust 的 `unwrap()` 说的是同一句。
两者**在 release 里都还在**,所以翻译保留这个检查,不换成别的。

**不是 `unwrap_or_default()`**。上游写 `!` 的地方,是它已经确认过值在那里;
把崩溃换成默认值,等于把一个响亮的失败换成一个安静的错答案。

一处区分:Dart 的**前缀** `!` 是布尔取反,**后缀** `!` 是非空断言,
两者拼写相同、含义毫无关系,所以 IR 里是独立节点而不是 `IrUnary('!')`。
`x++` / `x--` 也是后缀,但它们是伪装的赋值,归到可变性那一轮,这里拒绝。

### 测试:每条都配一个真的是 null 的搭档

否则"检查在"和"检查被换成了默认值"在永不为 null 的值上是同一个观察。

变异三个全杀,而且分得清是谁杀的:

| 变异 | 结果 | 杀手 |
|---|---|---|
| `!` 变成 `unwrap_or_default()` | 2 个测试失败 | **断言** |
| `!` 整个丢掉 | DOES NOT BUILD | rustc |
| `!` 当成布尔取反 | DOES NOT BUILD | rustc |

第一个是最重要的那个——它**能编译**,只是安静地给错答案,
只有断言能抓到。

### 结果

    analyzer  ok. 35 passed
    kernel    ok. 35 passed
    AGREE

普查:后缀 `!` **1294 → 0**,拒绝 11837 → **11154**,
成员 21127 → **21811**,零拒绝的类 778 → 819。

---

## 第 12 轮:可变性——`&mut self` 与它的传染

队头 1956 次里的第一半。按第 10 轮量到的分布,先做**穿过 `this` 的 6220 次**
(方法内改自己),它只需要 `&mut self`;穿过别的对象的 3869 次要可变接收者,
留给后面。

### "谁判断传染边界"——没有人判断,它是算出来的

先播种:写自己字段的方法。
再闭包:**在自己身上**调用了会改的方法的方法,也会改。
在类边界停下,因为调用别的对象已经因为别的理由被拒绝了。

**是不动点,不是一遍。** `outer` 调 `middle` 调 `bump`,只有 `bump` 写字段;
一遍只能找到 `middle`。

### 两处签名不归我们改,所以拒绝

- 变成 `impl std::ops::*` 的运算符——trait 规定接收者是 `self`
- 抽象基类声明过的方法——接收者属于 trait,改它要改整个 trait 和所有实现者

上游的运算符不赋值,所以第一条是护栏而不是损失。

### 一个 fixture "通过的理由是错的"

第一次跑变异,"传播只做一遍"**存活了**——因为我 fixture 里的声明顺序
恰好是 `bump → middle → outer`,一遍按声明序就够了。

把 `outer` 挪到 `middle` **前面**之后,同一个变异 DOES NOT BUILD。
**顺序就是这个测试本身。** fixture 里写清楚了为什么是这个顺序。

五个变异全杀(全都是 rustc 杀的——可变性错了根本编译不过,
这一点和 `!` 那轮相反,那里最重要的变异只有断言能抓)。

### analyzer 的一个坑

赋值左边的标识符,analyzer 的 `element` 是 **null**;元素在
`AssignmentExpression.writeElement` 上。读错地方的结果是语料里
每一次字段写都报 "assignment to `Null`"。

### `agree.py` 抓到一个我自己的错

Kernel 前端里 `object's` 的撇号没转义,Dart 编译不过。
analyzer 那侧 39 passed、Kernel 侧 FRONT END FAILED——
**两个前端的对照,抓到的是我在其中一个上犯的错。**

### 结果

    analyzer  ok. 39 passed
    kernel    ok. 39 passed
    AGREE

普查:"字段赋值" **1144 → 0**,拒绝 11154 → **10626**,
成员 21811 → **22339**,零拒绝的类 819 → 828。

---

## 第 13 轮:setter

**setter 是调用,不是写。** Dart 拼成 `a.x = 1`,Rust 拼成 `a.set_x(1)`,
而一个赋值到底是哪一种,取决于**接收者的类怎么声明的 `x`**——
所以这个区分必须在知道答案的地方做出,不能留给后端猜。
两个前端都已经能分辨(Kernel 直接给出目标成员,analyzer 靠 `isSynthetic`),
上一轮就是靠它拒绝 setter 的;这一轮把拒绝改成生成。

### 名字会撞

Dart 里 `get x` 和 `set x` **是同一个名字**,Rust 里不能是。
setter 变成 `set_x`,而且**所有按成员索引的东西都改成按 Rust 名索引**——
尤其是可变性集合。按 Dart 名索引会把 getter 和它的 setter 当成一个条目,
于是 getter 也被标成 `&mut self`。

fixture 里读取的测试**故意用不可变绑定**,所以这件事一旦做错就编译不过。

### 复合赋值要从 getter 读回

`fahrenheit += 18.0` 里的"当前值"来自 **getter**,不是字段——
`fahrenheit` 根本没有对应字段。

### 一个变异杀不掉,于是删掉那条规则

我原本写了"setter 天生就是 mutating,即使它只是往下委托"。
变异把这句去掉之后,**测试全过**——因为只委托的 setter 会被传染分析抓到,
而什么都不写的 setter 本来就不需要 `&mut self`。

**一条没有可观察差别的规则,不该被写下来。** 删了,并在原地写明为什么。

其余三个变异全杀(名字撞、setter 调用不传染、setter 返回值而不是 `()`)。

### 结果

    analyzer  ok. 44 passed
    kernel    ok. 44 passed
    AGREE

普查:setter **812 → 0**,拒绝 10626 → **9939**(**首次跌破一万**),
成员 22339 → **23025**,零拒绝的类 828 → 838。

---

## 第 14 轮:枚举

先量,结果是**一件事而不是两件**——但比例悬殊到值得只做一半:

| | plain | enhanced |
|---|---|---|
| `package:flutter` | **232** | 17 |
| `package:gallery` | **26** | 1 |

约 1001 个枚举值。**93% 是简单 enum**,而简单 Dart enum 就是 Rust enum,
两种语言在这里什么都不用说。增强 enum(带字段和方法)是 Rust enum **加一个 impl**,
是另一件事,**拒绝并说明**,不当成普通的生成——那会把它的方法悄悄丢掉。

### 变体要改名,别的都不改

`Axis.vertical` → `Axis::Vertical`,**只改首字母**。
其余照抄,理由和私有成员保留下划线一样:改了就没法和上游对照搜。

fixture 里放了 `spaceBetween` 这种多词值,因为单词值无论改不改名都能过。

### `Copy` 不是装饰

没有它,译出来的函数体里读两次 `self.axis` 就会从 `&self` 里 move 出去。
测试里单独验了这一条。

### 两个前端的一处不对称

analyzer 的 AST 里 **enum 不是 `ClassDeclaration`**,所以
`lowerLibrary` 从来没看过它们——847 次"枚举值引用"被拒绝,
而它们的**声明**前端根本没读到。Kernel 那边 enum 就是 class(带 `isEnum`),
天然会走到。这一轮把 analyzer 补齐。

引用侧还有一处:`Axis` 解析成 `EnumElement` 而不是 `ClassElement`,
这正是那 847 次拒绝的直接原因。

### 一个变异瞄错了,不算等价

"增强 enum 当普通的生成"第一次**存活**。查下来:我的变异只删掉了
**报告**,而 `values:` 那一行是另一道闸,产出根本没变——
**是瞄错,不是天然等价**。改瞄那道闸之后它被杀死。

在此之前还补了一个测试:`Season` 被拒绝这件事**没有任何测试碰过**。
断言的是"编译器生成了什么",而且**没有 `Season` 类型可供断言**——这正是重点——
所以用 `include_str!` 直接读生成的文件。**测不到的防护等于没有防护**,
第 6 轮同一条。

四个变异全杀。

### 结果

    analyzer  ok. 48 passed
    kernel    ok. 48 passed
    AGREE

普查:枚举引用 **847 → 0**,拒绝 9939 → **9392**,
成员 23025 → **23575**,零拒绝的类 838 → 870。

---

## 第 15 轮:那 891 是三件事,做了最便宜的一件

上一轮的疑问:891 次"getter 引用未解析"是记账问题还是真缺口?
**都不是——它是三件事被记成了一件。**

先改尺子。原来的拒绝信息是 `identifier of GetterElementImpl`,
报的是 **analyzer 内部类的名字**,对"该做什么"毫无用处。
改成报**它声明在哪**之后:

| 次数 | 是什么 |
|---|---|
| 391 | **mixin 上的成员**——mixin 还没建模 |
| 390 | **类上的方法引用**(撕下来当值用),`_identifier` 只处理访问器和字段 |
| 507 | **顶层常量**(241 个不同的),外加 964 次顶层函数调用 |

不同的阻塞项从 266 涨到 510——**同一批拒绝,分辨率高了一倍**。

### 做顶层常量

Dart 有模块级名字,Rust 也有,两边都不需要 owner。

区分靠的还是那条老规矩:**analyzer 把顶层 `const` 建模成合成 getter**,
所以合成的是存储的常量,非合成的是计算的 `get foo => ...`——后者是函数,拒绝。
Kernel 那边看 `isConst || isFinal`。

### 前端的拒绝以前不进文件

`usesComputed` 被拒绝了,但**生成的文件里一个字都没有**——
前端拒绝只报到 stderr,而后端拒绝一直会留 `// NOT TRANSLATED`。
**拿着文件的人不该还得留着当时的控制台。** 现在两种拒绝都进文件。

这是写测试时发现的:我照着后端的格式去断言,断言失败了。

### 两个变异瞄错了

"增强 enum"那次(第 14 轮)之后又来一次:
"前端拒绝不进文件"这个变异我改的是**头部那行**,
而测试断言的是**逐条那行**——产出里仍然有标记,所以存活。
瞄准逐条那行之后被杀死。

**连续两轮都是我瞄错,不是代码等价。** 变异存活时,
第一件要查的是"我改的这行,和测试断言的是不是同一件事"。

三个变异全杀。

### 结果

    analyzer  ok. 52 passed
    kernel    ok. 52 passed
    AGREE

普查:拒绝 9392 → **9155**,成员 23575 → **23814**,零拒绝的类 870 → 878。

---

## 第 16 轮:给 Kernel 也造一把尺子,队头整个换了

普查一直只跑 **analyzer** 前端。那是它写下来时唯一的前端,而现在**发布路径是 Kernel**——
所以它印出来的队列,是**另一个编译器的**队列。

两个前端共享 IR 和后端,但**它们看到的不是同一个程序**:
Kernel 拿到手时 mixin 已展开、super 已解析、常量已求值,
所以一个前端的真阻塞项,在另一个前端可能根本不存在。**只有两边都量才知道。**

### 队头完全不同

| Kernel | 次数 | analyzer |
|---|---|---|
| `EqualsNull` | **2505** | 不存在(它看成普通二元表达式) |
| `Let` | **2046** | 不存在(CFE 自己的降级临时量) |
| super 构造调用带参数 | 1297 | 421 |
| 闭包字面量 | 746 | 900 |
| 具名参数无被调方 | 526 | 不存在 |
| `BlockExpression` | 340 | 不存在 |

拒绝总数 **13317(Kernel)vs 9155(analyzer)**,类 3665 vs 3331。

**上轮那 391 个 mixin 成员在 Kernel 侧彻底消失了**——CFE 展开了 mixin,
猜测成立,而且是白得的。

### 尺子自己先出了个 bug

第一版把**类名**当成了类别——Kernel 前端的拒绝带 `ClassName: ` 前缀,
而 `category()` 取第一个 `': '` 之前的部分。印出来是"翻译不了的类名排行榜"。

改成**按类调用 `lowerClass`**,不去把信息从字符串里解析回来。
(我在第一版的注释里还专门为"不解析"辩护过,然后就解析了。)

### 一个我自己的 bug,只有这把新尺子看得见

`const instance missing \`#index\`` **1125 次**——第 14 轮声称做完的枚举。
Kernel 里枚举值是 `InstanceConstant`,带 CFE 自己的 `#index` 和 `_name` 字段,
而我拿它去走构造函数参数。**analyzer 前端根本遇不到这个形状**,所以两轮都没发现。

修了:拿 `_name` 的字符串值生成 `Enum::Variant`。
拒绝 14064 → **13317**,零拒绝的类 686 → **723**,成员 21963 → **22712**。

### 这个修复没有回归测试,记下来

`alignment.dart` 里没有枚举常量引用,所以 `agree.py` 覆盖不到它。
我拿 `painting/borders.dart` 直接验证了(生成出 `BorderStyle::None`,
`missing #index` 归零),**但那是一次性检查,不是测试**。

**Kernel 独有的路径现在没有回归通道**——fixture 走 analyzer,
`agree.py` 只盯一个库。这是下一轮要补的。

---

## 第 17 轮:fixture 也编成 dill,两个前端并排跑

上一轮的问题:**Kernel 独有的路径没有回归通道**。
fixture 是源码文件,只走 analyzer;`agree.py` 只比一个上游库。
所以 Kernel 侧的 bug 可以存活任意多轮——而且已经存活过了。

### 先验证能不能编

fixture 没有 `main`,frontend_server 不干。
不给 fixture 加 `main`(那就成了被翻译内容的一部分),
改成生成一个只做 `import` 的包装入口——frontend_server **不做 tree-shaking**,
import 就足以把库放进 dill。每个 fixture 约 10 MB(带着 dart:core),进 scratch 不进仓库。

### 当场抓到三个真 bug

工具第一次跑,九个 fixture 里四个不一致:

1. **具名参数的默认值**:Kernel 侧生成 `weigh()`——**一个参数都没填**,
   而函数有三个。这正是我第 2 轮为 analyzer 修过的 bug,
   `InstanceInvocation` 没把被调方传给 `_arguments`。
   **同一个 bug 在另一个前端活到了第 17 轮**,因为没有东西比较过两者。
2. **增强枚举**:Kernel 侧把 `Season` 当普通 enum 生成,**丢掉了它的方法**——
   正是 analyzer 侧拒绝的那件事。第 14 轮的测试用 `include_str!` 只读了
   **analyzer 的输出**,Kernel 侧从没被看过。
3.(第三个是上一轮修的枚举常量,这一轮它有了回归通道。)

修完:九个里七个逐行相同。

### 剩下两个是差异,不是 bug——让 fixture 自己说

- `nullcheck`:Kernel 把 `??` 降成 `Let`,还没建,**正确地拒绝**而不是错译。
- `toplevel`:**Kernel 的常量求值器把 `kSpacing * 2.0` 折叠成了 `8.0 * 2.0`**,
  常量的名字在到达编译器之前就没了。正确,但比 analyzer 的产出难读——
  **这是 dill 路线的又一项真实代价**(第一项是没有文档注释)。

一把总在报失败的尺子会没人看,所以 fixture 顶部写 `// DIFFERS: 原因`,
理由就放在展示它的那个 fixture 上。反向也检查:
**声明了差异却没差异,同样报错**——要么差异被修好而注释过期了,
要么这个 fixture 不再考察它本来要考察的东西。

### 结果

九个 fixture 全部有交代:七个逐行相同,两个是已声明的差异。
测试 52 个全过,`agree.py` AGREE。
Kernel 普查:拒绝 13317 → **13045**,零拒绝的类 723 → **732**。

---

## 第 18 轮:`x == null`,以及把 `Let` 量清楚

### `x == null` 的要点是两个前端必须落到同一个节点

Rust 问这个问题的方式不同:可空值是 `Option`,测试是 `x.is_none()`,
而不是和一个不存在的 null 比较。

关键不在于怎么翻,在于**两边要落到同一个 IR**:
Kernel 给的是 `EqualsNull`(CFE 已经认出了这个形状),
analyzer 给的是普通的 `==` 对 null 字面量。**放着不管,同一份 Dart 会译出两种 Rust,
而这样的地方有 2524 处。**

### 一个变异存活,而杀它的是另一把仪器

"只检查右操作数"这个变异**通过了所有 57 个测试**——因为
`None == self.other` 在 Rust 里和 `self.other.is_none()` **行为等价**。

但它让**两个前端产出不同的 Rust**(Kernel 会把 `null == other` 也归一化),
`fixtures.py` 报 `2 lines differ`。单独验过:基线 same,变异后 differ,还原后 same。

**这条守卫由前端对照守着,不由断言守着。** 记清楚是哪把仪器在守它——
第 11 轮 `unwrap_or_default` 那个只有断言能抓,这个反过来。

其余两个变异被断言杀死(`is_none`→`is_some` 挂 5 个,`!=` 丢负号挂 1 个)。

### `Let` 量清楚了:77% 是两个可辨认的构造

`package:flutter` 里 **14946 个 `Let`**(比 2077 次拒绝多得多,
因为很多嵌在已被拒绝的成员里):

| 次数 | 形状 |
|---|---|
| **6764(45%)** | `a ?? b` |
| **4838(32%)** | 基于 null 测试的条件式(多半是 `a?.b`) |
| 1580 | `let then VariableSet`(CFE 的模式匹配降级) |
| 823 | block expression |
| 373 | 嵌套 let |

**答案是还原,不是直译。** `a ?? b` 应该译回 `a ?? b` 的形态,
而不是 CFE 拼它用的那三条语句。第 8 轮把运算符从 `InstanceInvocation`
还原成二元表达式,是同一个判断,而且那一轮证明了还原更好——
直译会产出没人认得的 Rust,而这个项目的产出要能和上游对照着读。

### 结果

十个 fixture:八个逐行相同,两个是已声明的差异。测试 57 个全过。
Kernel 普查:`EqualsNull` **2524 → 0**,拒绝 13045 → **12151**,
零拒绝的类 732 → **765**,成员 22907 → **23806**。

---

## 第 19 轮:从 `Let` 还原 `??`,并修一个潜伏了十七轮的 bug

### 先量,量出来的是个 bug

`unwrap_or(b)` **无条件求值 `b`**,而 Dart 的 `??` 是短路的。
后端从**第 2 轮**起就一直用 `unwrap_or`。量了才知道范围:

`package:flutter` 的 6764 个 `??` 里,右边**只有 23% 是字面量或常量**。
其余是调用、构造函数,还有**六处是 `throw`**——那里 eager 求值不是给出错答案,
是**每次都抛异常**。

所以字面量保留短形式(可读且可证明安全),其余一律 `unwrap_or_else(|| b)`。

### 嵌套 `??` 逼出了 IR 里缺的东西

`value ?? second ?? 2.0`,其中 `second` 本身可空。
`a ?? b` 的结果**当且仅当 `b` 非空时非空**,而 Rust 把两者拼得不一样:
`unwrap_or_else` 产出值,`or_else` 产出 `Option`。

IR **不带表达式类型**,这是第一次真的需要它。加了 `IrIfNull` 节点,
带 `nullableResult` 和 `eager` 两个标志,**都由前端填**——它们知道类型,IR 不知道。
Kernel 那边 `ConditionalExpression` 自带 `staticType`,不用建类型上下文。

四种拼法各有其位:

| 结果可空 | 右边纯 | Rust |
|---|---|---|
| 否 | 是 | `a.unwrap_or(b)` |
| 否 | 否 | `a.unwrap_or_else(\|\| b)` |
| 是 | 是 | `a.or(b)` |
| 是 | 否 | `a.or_else(\|\| b)` |

**这个 bug 是 fixture 里的嵌套用例逼出来的**,而它不编译——写对之前根本过不去。

### fixture 要能观察到"求值了没有"

`boom()` 里放 `assert(false)`:右边被求值就 panic,不被求值就静默。
**默认值如果是个普通数字,这个 fixture 分辨不出两种形式。**
(第一版我写的是 `throw`,但 `throw` 还没建——**需要未支持构造的 fixture 什么都测不了**,改用 assert。)

### 反向检查抓到一条过期声明

`nullcheck` 上一轮声明"两个前端应当不同"(Kernel 把 `??` 降成 `Let`)。
`??` 还原之后**它们相同了**,工具报
`SAME, but the fixture expects a difference`。声明已删。

### 变异三个全杀,第三个仍是前端对照抓的

"不识别 `??` 形状"这个变异让 **cargo test 全过**(analyzer 侧不受影响),
`fixtures.py` 报 9 lines differ。**连续两轮,Kernel 侧的守卫由前端对照守着。**

### 结果

十一个 fixture:十个逐行相同,一个是已声明的差异(Kernel 折叠常量)。
测试 61 个全过,`agree.py` AGREE。
Kernel 普查:`Let` 2525 → **1573**,拒绝 12151 → **11690**,
零拒绝的类 765 → **802**,成员 23806 → **24272**。

---

## 第 20 轮:从 `Let` 还原 `?.`

先 dump,猜测被证实(第 8、18、19 轮都是这个顺序,四次 IR 都一次对上):

| 次数 | 形状 |
|---|---|
| 2318 | `a?.b` 字段/getter |
| 1899 | `a?.foo()` 方法调用 |
| 436 | `?.` 调函数值 |
| 89 | 链式 `a?.b?.c` |
| 44 | `?.` 字段写 |

**`??` 和 `?.` 在 Kernel 里长得像但不重叠**:
`??` 的**临时变量在 else**,`?.` 的 **null 在 then**。fixture 里把两者并排放,
就是为了让混淆能显形。

### IR 需要一个"被绑定的值"

Rust 说这件事用 `a.map(|it| ...)`,所以函数体需要一个名字指代被绑定的值。
加了 `IrBound` 当那个名字。

**函数体保留成表达式**,而不是拆成"成员 + 参数"——
这样链式 `a?.b.c()` 才能只绑一次、对绑定做两件事,而上游有 89 处是链式的。

闭包参数用**固定名字 `it`**:链式会嵌套闭包,内层遮蔽外层,
而那正是 Dart 的意思——内层访问针对的就是内层的值。

### fixture 要能观察"跑了没跑"

`boom()` 里 `assert(false)`:body 被求值就 panic。
**body 如果只是个普通读取,这个 fixture 分辨不出"跳过了"和"跑了但答案碰巧一样"。**
和上一轮 `??` 是同一族风险,STATUS 里那条常备警告直接用上了。

### 变异三个全杀,第三个还是前端对照抓的

"Kernel 侧不识别 `?.` 形状"让 **cargo test 全过**(analyzer 侧不受影响),
`fixtures.py` 报 12 lines differ。**连续三轮如此。**

### 结果

十二个 fixture:十一个逐行相同,一个是已声明的差异。
测试 66 个全过,`agree.py` AGREE。
Kernel 普查:`Let` **从队头掉到第六**(1573 → 426),
拒绝 11690 → **11025**,零拒绝的类 802 → **814**,成员 24272 → **24939**。

---

## 第 21 轮:super 构造函数——把继承扁平化

先量,因为这一条把两种情况记成了一条:

| | 次数 |
|---|---|
| **基类是抽象的** | **1520(80%)** |
| 基类是具体的 | 368 |
| 基类**没有自己的字段** | 754(40%) |
| 最深的带字段基类链 | **6 层** |

80% 的基类是抽象的,而第 5 轮把抽象类做成了 trait、**trait 不持有存储**,
当时把基类字段记成 `NOT TRANSLATED`。**那笔账在这一轮到期。**

### 扁平化,分三处

1. **字段**:子类的 struct 带上所有基类的字段(`_allFields`,递归)。
2. **构造函数**:基类构造函数被**内联**——它的参数换成 super 调用传的实参,
   它的字段初始化式并进子类的。也递归,因为基类自己也可能调 super。
3. **trait 里的读**:基类的方法体读 `width`,而字段现在在实现者身上。
   所以 **trait 为它的每个字段声明一个访问器**,每个实现者提供
   `fn width(&self) -> f32 { self.width }`。第 5 轮那条
   `// NOT TRANSLATED: 字段` 的注释被这个取代了。

### 两处是编译器告诉我的

- `impl Shape for Rectangle` **一开始根本没生成**:那个 impl 只在"有必需方法"时才发,
  而现在**有字段访问器也需要它**。没有它,基类的方法从子类根本够不着。
- `Padded extends Square extends Shape`,而 `Square` 是**具体类**。
  只看直接基类的话 `Padded` 什么都不实现。改成**为每一个抽象祖先**生成 impl。

### 变异四个全杀,最后一个只有断言抓得到

前三个(不扁平化字段、不做参数替换、只看直接基类)**都编译不过**。
第四个——**参数配对反序**——**编译得过**,只有断言抓到:
fixture 里 `Square(side) : super(side, side)` 两个参数相同,分辨不出,
所以 `Rectangle(w, h) : super(w, h)` 那条测试才是杀它的。
**fixture 里既要有"两个参数相同"的用例,也要有"不同"的。**

### 结果

十三个 fixture:十二个逐行相同,一个是已声明的差异。
测试 69 个全过,`agree.py` AGREE。
Kernel 普查:super 构造调用 **1481 → 0**(掉出前六),
拒绝 11025 → **10838**,零拒绝的类 814 → **846**,成员 24939 → **26286**。

---

## 第 22 轮:闭包——先量,做能做的一半

| | flutter | gallery |
|---|---|---|
| **捕获 `this`** | **2522(60%)** | 611(45%) |
| 什么都不捕获 | 794(19%) | 542(40%) |
| 只读外层局部 | 777(18%) | 193(14%) |
| 赋值外层局部 | 138(3%) | 0 |

本轮做前两类。**捕获 `this` 的拒绝,而且说清为什么**:
它比调用它的那次调用活得久,而 `this` 是个借用——
那是所有权安排,不是翻译,单独一轮。

### 闭包和函数类型是一件事

闭包总要传给某个参数,而参数类型原来生成的是无效的 `Function`。
所以 `IrType` 加了**结构化的函数类型**,后端按位置选拼法:

- **参数**是借用位置 → `impl Fn(f32) -> f32`,闭包字面量可以直接传
- **字段或返回值**是拥有位置 → `Box<dyn Fn(f32) -> f32>`

还顺带做了**调用函数值**(`f(x)`)——闭包不能被调用就没有用。

### 一个守卫抓不到它声称要抓的东西

analyzer 侧我一开始用 `toSource().contains('this')` 判断闭包有没有捕获 `this`。
**Dart 允许不写 `this` 就用实例成员**——`factor` 在方法里就是 `this.factor`——
所以这个检查**恰好放过了它要拦的那些**。
Kernel 侧用的是真正的 `ThisExpression` 查找器,于是两个前端会不一致。
改成解析标识符,这是唯一能问对的方式。

### 两个变异瞄错,两条同样的教训

- **"闭包参数乱序"存活**:fixture 里每个闭包**只有一个参数**,反转是空操作。
  和第 21 轮 `super(side, side)` 一模一样。补了个 `(a, b) => a - b`
  (10 - 3 = 7,反过来是 -7)之后被杀。
- **"函数类型在参数位被装箱"存活**:我只替换了第一处,而那不是方法签名那条路径。
  全部替换后 DOES NOT BUILD。

**这已经是第三、第四次同类失误了**,写进常备条目。

### 结果

十四个 fixture:十三个逐行相同,一个是已声明的差异。
测试 74 个全过,`agree.py` AGREE。
Kernel 普查:闭包字面量 **974 → 0**(剩 878 是"捕获 `this`"),
调用函数值 **740 → 0**,拒绝 10838 → **10329**,
零拒绝的类 846 → **855**,成员 26286 → **26804**。

---

## 第 23 轮:级联,以及三个静默出错的地方

先 dump `BlockExpression`(450),分成两族:约 800 是**级联**
(`Paint()..color = c` —— 绑定、改、返回绑定),约 470 是 **switch 当表达式**
(被 `LabeledStatement` 包着,要先有 switch)。做级联。

Rust 说这件事就是块表达式:`{ let mut it = ...; it.color = c; it }`。
两种语言在这里对得上,所以是翻译不是编码。

### 探针分对了族,却分错了形状

探针把"绑定"当成块的**第一条语句**,而 Kernel 里级联的真实形状是
**`Let` 包着 `BlockExpression`**——绑定在 `Let` 上。
按探针的形状写完,Kernel 侧一个级联都没认出来。

**是 fixture 的两前端对照抓到的**,不是探针。
探针分对了"这是什么",分错了"它长什么样"。

### 三处静默出错,两处修了一处拒绝

1. **`&mut self` 传染错了**:级联写的是**刚绑定的局部**,不是 `self` 的字段,
   而 `_WalkSelf` 不看 target,于是每个含级联的方法都变成了 `&mut self`。
2. **`Paint` 的字段在声明处初始化**(`double width = 0.0;`),构造函数不设它们,
   于是 257 次"field never initialised"。现在 `IrFieldDecl` 带 `initial`,
   **构造函数优先,声明值兜底**——Dart 就是这个顺序。
3. **构造函数体被整个丢掉**:`Tinted(v) { opacity = v; }` 生成出来
   `opacity: 1.0`,**参数根本没用上,而且编译得过**。
   这是最坏的一类。按老规矩**拒绝**它,不悄悄丢。

### 一个变异存活,又是 fixture 分辨不出

"声明值压过构造函数"存活——因为 fixture 里**没有一个字段同时有两者**,
顺序从未被检验。补了 `Tinted(v) : opacity = v` 且 `double opacity = 1.0`
之后被杀。**第五次同类失误**(前四次:单参数闭包、相同的 super 参数、
只替换第一处、没人碰 `Season`)。

### 另一个 fixture 的前提过期了

`supercalls` 里那个"故意不可翻译"的基类方法用的是级联——级联做通之后
它可以翻了,于是两个前端分岔。**换成 `for` 循环**(两边都还不支持)。
**声明"这个东西不可翻译"的 fixture,会随着编译器进步而失效**,
而两前端对照是发现这件事的仪器。

### 结果

十五个 fixture:十四个逐行相同,一个是已声明的差异。测试 79 个全过。
Kernel 普查:`BlockExpression` 掉出前六,拒绝 10329 → **9997**
(**首次跌破一万**),零拒绝的类 855 → **912**,成员 26804 → **27036**。

---

## 决定:异常翻译到 `Result<T, E>`

用户定的方向,并明确说明理由:**约定到返回值的异常处理优于 panic**。
我先前提出的传染问题是**要处理的成本,不是反对的理由**,这里记下来。

语料(第 22 轮之后量的):

| | flutter | gallery |
|---|---|---|
| `throw` | **831** | 94 |
| `try/catch` | 171 | 1 |
| `on <Type> catch` | 174 | 1 |
| 带 stack trace 的 catch | 133 | 1 |
| `try/finally` | 73 | 0 |
| `rethrow` | 8 | 0 |

抛的是什么:`FlutterError` **474**、`StateError` 135、`UnsupportedError` 64、
`UnimplementedError` 45、`NoSuchMethodError` 39。

**`Result` 路线要面对的三件事**,都得先量再做:

1. **传染范围。** Dart 的 throw 不出现在签名里,所以"哪些函数返回 `Result`"
   要**算**:从每个 throw 出发,沿调用图向上传播,在有 `try/catch` 的地方停下。
   这和第 12 轮 `&mut self` 的不动点是同一形状的分析,只是跨类。
2. **错误类型。** 474 次 `FlutterError` 说明有一个主导类型。
   可以先做 `Result<T, FlutterError>`,别的 `throw` 暂时拒绝——
   **先量"一个函数能抛几种类型"**,如果多数是一种,枚举就不必要。
3. **`try/finally`(73)。** `Result` 不管清理。Rust 的答案是 `Drop` 守卫,
   或者把清理写在两条路径上。这一条和 `Result` 是**分开的**问题。

**不要**把译不了的 throw 变成能编译的安静错答案——第 19 轮 `??` 的教训。

## 第 24 轮:`throw` → `Result<T, E>`

先量传染范围,三个数都很硬:

| | |
|---|---|
| 直接 throw 的成员 | 717 |
| **传播后返回 `Result` 的** | **5906(占全部成员 20%)** |
| 被 `try` 挡住的 | **只有 20** |
| **只抛一种错误类型的成员** | **709 / 721(98%)** |

三条结论各自决定了一件事:

1. **20% 不是"几乎全部"**,所以这条路走得通——这正是决定之前该知道的数。
2. **98% 只抛一种类型**,所以错误类型是每个函数一个具体类型,
   **不需要错误枚举**。抛两种的 12 个成员**拒绝**,不为 2% 造一个会渗进
   所有签名的枚举。
3. **`try/catch` 几乎挡不住传播**(5906 里只挡了 20),
   所以不能指望它当边界——传播基本是一路到顶的。

### 实现

"哪些函数返回 `Result`"是**算出来的**,不是写出来的:
以 throw 的方法为种子,沿类内调用做不动点,和第 12 轮 `&mut self` 同形状。
调用点补 `?`,普通 `return v` 变 `Ok(v)`,`throw e` 变 `return Err(e)`。

**在类边界停下**,和别的东西一样卡在同一堵墙上:20% 是整程序的数,
这里算的是一个文件里看得见的那部分。

### 三个我这一轮自己造的 bug,都是 fixture 对照抓的

1. **"拒绝构造函数体"把所有构造函数都拦了**:CFE 给每个构造函数一个 body,
   所以"有没有 body"问错了,该问"里面有没有语句"。
2. **Kernel 侧的 `throw` 检查排在通用 `ExpressionStatement` 之后**,
   于是从没被用到,每个会抛的方法都被拒绝。analyzer 侧是对的,
   **两前端对照立刻显形**。
3. bash 把测试注释里的反引号当命令替换执行了(那个老陷阱),
   写坏了 `lib.rs`。改用 Write 工具。

### 拒绝数升了,那是诚实

9997 → **10040**,零拒绝的类 912 → 900。原因是**构造函数体从静默丢弃
变成了明确拒绝**——上一轮发现 `Tinted(v) { opacity = v; }` 会生成
忽略参数的构造函数,而且编译得过。**数字变差,尺子变准**,和第 9 轮同理。

### 结果

十六个 fixture 全部逐行相同(那条"Kernel 折叠常量"的差异也消失了)。
测试 83 个全过,`agree.py` AGREE。三个变异全杀。

---

## 第 25 轮:`try/catch`,以及一条早该发现的假账

先量,三个数决定了实现形状:

| | |
|---|---|
| catch 子句 | 174 |
| **`catch (e)`,不带类型的** | **155(89%)** |
| 绑定了 stack trace 的 | 133(76%) |
| 多于一个子句的 try | 2 |
| `try/finally` | 73 |

所以:**不为 2 个多子句造类型匹配**(拒绝),**stack trace 绑了不读就当没有**
(Result 不带 stack,读它拒绝),`try/finally` 是另一件事(`Drop` guard)。

### 一个决定是承重的:try 体进闭包

`?` 是**从它写在的那个函数返回**。直接写在方法体里,它会越过本该接住它的
catch 一路返回出去——而且**编译得过**。所以 try 体发成一个立即调用的闭包:

```rust
match (|| -> Result<(), RangeError> {
    result = self.checked(value)?;
    Ok(())
})() {
    Ok(()) => {}
    Err(e) => { result = -1.0; }
}
```

闭包的错误类型**取自 try 体**,不是取自外层方法:会 catch 的方法自己不失败,
没有自己的错误类型,`Result<(), _>` 推不出来(E0282,真撞上了)。

### 同一个闭包会吞掉别的东西:10/64 的 try 体里有 `return`

闭包接住 `?` 的同时也接住了 `return`——那个 `return` 会从闭包返回,方法接着
往下走。**这也编译得过**,是静默错答。量了:`package:flutter/` 64 个
try/catch 里 **10 个(16%)体内有 return**,不罕见。两个前端都**拒绝**。
(handler 里的 `return` 是对的:它就发在方法体的 match 臂里。11 个。)

### 尺子上的窟窿:拒绝本身没人看着

`fixtures.py` 比对前先把注释全剥掉,而拒绝就是一条注释。也就是说
**删掉一条拒绝规则,没有任何检查会变红**——而拒绝规则正是拦住"编译得过的
错答"的东西。fixture 现在自己声明:

    // REFUSES: return inside a try body

两个前端的输出里都必须出现这条。加了之后,"删掉 return 拒绝"这个变异才被杀掉。

### 上一轮的 10040 是假账

这一轮想报"拒绝数变化"时,发现对不上。用 `git worktree` 把**上一个提交**放到
**同一个 dill** 上重量:

|  | HEAD(第 24 轮) | 这一轮 |
|---|---|---|
| 零拒绝的类 | 781 | 781 |
| **发出的成员** | **7805** | **7806** |
| 拒绝总数 | 6793 | 6792 |

上一轮记的 `9997 → 10040` 复现不出来。那条记录旁边写着 `664 库 / 3665 类`,
而今天:`package:flutter/` 是 525/2743,`package:` 是 928/3979,全库是 950/4969
——三个 app.dill × 三个前缀,**没有一个组合给出 664/3665**。那个数来自
哪个输入已经追不回来了,所以它**不能和任何后续数字比较**,包括这一轮的。

教训不是"数字错了",是**数字没带量它的条件**。从今往后 STATUS 里的普查数字
必须带 dill 路径和前缀。本轮是:

    .dart_tool/flutter_build/ef21e168…/app.dill   前缀 package:flutter/

顺带:**try/catch 只换来 1 个成员**。普查按"每个成员卡在第一个 blocker"计,
所以拿掉一个 blocker 多半只是露出下一个。try/catch 的价值不在解锁成员,
在于**掐断 Result 的传染**——那件事普查量不到。

### `agree.py` 也是假账,而且红了两轮

第 24 轮记的 "agree.py AGREE" 复现不出来。在 HEAD 上、在同一个 dill 上、在
另外两个 dill 上,全是 `DISAGREE` + `kernel DOES NOT BUILD`。红的理由是具体的:

    error[E0425]: cannot find function `alignment_geometry_super_to_string`

**悬空调用**,和第一轮 `_stringify` 同宗:基类的 `toString` 自由函数没发出来
(trait 默认体退化成 `todo!()`),但子类的 `super.toString()` 照发不误。
`_superCall` 问的是"基类 IR 里有没有这个方法",而该问"**那个自由函数发得出来吗**"。

修法是**用同一段代码试发一遍**再看结果,而不是再写一条判断规则——
第二条规则是会和第一条吵架的东西。`_superFailed` 是每个类一份的,子类的
backend 看不到基类那份,所以补了一个带缓存的试发。

修完 `E0425` 没了,`agree.py` **仍然 DISAGREE**,但换了理由,而且是本来就知道的
那个:Kernel 侧不翻译 `Alignment` 的常量和运算符(普查里 `const instance
missing dx/value/…` 合计几百条排前十)。**这条留着红**,它现在报的是真事。

### 结果

十七个 fixture 全部两前端一致;测试 86 个全过;四个变异全杀,且都死在该死的线上
(先跑出来一个"杀了但死错地方"的,改了报错取行才看清)。

---

## 第 26 轮:从 try 里出来的控制流,和构造函数已经不在了的常量

普查(`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`):

| | 第 25 轮 | 这一轮 |
|---|---|---|
| 零拒绝的类 | 781 | **820** |
| **发出的成员** | 7806 | **8721(+915)** |
| 拒绝总数 | 6792 | **6182(−610)** |
| 不同的 blocker 种类 | 828 | 1058 |

blocker 种类**变多了**,和第 24 轮同理:一条笼统的拒绝碎成了许多条具体的。

### 用户指出上一轮的拒绝是我实现形状的毛病,不是语义做不到

他说得对。`Result<T,E>` 当然能捕获——它本来就在捕获。被拒的两件事都能修:

**1. try 体里的 `return`(10/64)。** 我把 try 体放进闭包好让 `?` 停在 catch,
代价是 `return` 也停在闭包里。修法是让闭包**携带控制流**:

```rust
match (|| -> Result<Option<f32>, RangeError> {
    return Ok(Some(self.checked(value)?));
    #[allow(unreachable_code)] Ok(None)
})() {
    Ok(Some(v)) => return v,
    Ok(None)    => {}
    Err(e)      => { /* handler */ }
}
```

`Ok(None)` 那一臂在"每条路都 return"时够不到,但 Rust 要它有类型,所以加了一个
**保守的必然返回判定**——说"是"错了会 panic,说"否"错了只是多一条编译器会抱怨的
`{}`,所以只在看得见的地方说是。

**2. `try/finally`(73)。** 我在 STATUS 里写过"Rust 的答案是 `Drop` 守卫"。
不对。上面那个 match **本来就是所有出口的唯一汇合点**,finalizer 在 dispatch
之前跑一遍即可。`Drop` 反而是错的:守卫的 `drop` 里既不能 `?` 也不能 `return`,
而 dispatch 两样都要。

**还剩一件真做不到的**,而且不是 `Result` 的锅:Dart 的 `catch (e)` 抓一切,
包括越界和溢出;翻译后这些是 Rust 的 panic,`match` 一个也接不住。要补只有
`catch_unwind`,或者把每个会失败的原始操作都翻译成返回 `Err` 的检查版本
——后者是对的,代价是所有算术和索引都进 `Result`,那要先量。

### 常量实例:构造函数这条路本身不成立

先量。5602 个非枚举 const 实例,**只有 1021 个**(18%)重建得出来。四种失败:

| | |
|---|---|
| **类里一个构造函数都没有** | **2965(53%)** |
| 只有具名构造函数(`EdgeInsets` 5 个) | 224 + 137 + 92 + 54 |
| 字段被父构造函数改了名(`Offset` 存 `_dx`) | 272 + 164 |
| 构造函数重定向(`Duration`、`Color`) | 249 + 249 |
| 初始化列表算出来的字段(`TextStyle`) | 76 + 40 + 18 |

第一条是决定性的:`_Linear` 在 dill 里**真的没有构造函数**。
因为 **const 实例从不调用构造函数**,它不可达,被摇掉了。
所以"重建成构造函数调用"依赖一个编译器有权删掉的节点——**它是优化,不是答案**。

答案是 `InstanceConstant` 永远带着的、**已经算好的字段值**:发结构体字面量。
构造函数还在且形参一一对上字段时仍走构造函数,那是为了保住两个前端说同一句话。

只对**本文件里的类**发字面量:字面量要点名字段,而我们只有资格点自己写出来的
那些——`Duration { _duration: … }` 会点到手写桩的字段上,哪天桩换个拼法就静默错。

### 两个存活的变异,都是等价的

- 强制走字段回退,`constinstance` 的断言全过——**因为字面量按值也是对的**。
  补了 `constdirect.dart`(不声明 DIFFERS)才杀掉:那里两个前端必须逐字一致。
- 那条 `names.length != byName.length` 守卫,在 6183 次拒绝里**只影响 1 次**,
  且被后面的 null 检查提前拦下。**量完之后删掉**,不留不可测的代码。

### 结果

十九个 fixture 两前端一致,93 个测试全过,五个变异全杀。

---

## 第 27 轮:四个 blocker 一次做完(`Let`、`for`/`while`、`identical`、字符串拼接)

普查(`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`):

| | 第 26 轮 | 这一轮 |
|---|---|---|
| 零拒绝的类 | 820 | **939** |
| **发出的成员** | 8721 | **9845(+1124)** |
| 拒绝总数 | 6182 | **5067(−1115)** |

### 最大的收获是发现之前在绕远路

`Let` 是最大的 blocker(1382)。之前的做法是一个个认模式——`??`、`?.`、级联。
量了一下:**14476 个 `Let` 里,剩下的没有任何模式可认**,例子清一色是
`let #0 = <实参> in new Widget(…, #0, …)`,就是 CFE 给实参绑临时变量。

而 `Let` 本来就是 let 绑定。Rust 的块表达式 `{ let t = init; body }` **一条规则全覆盖**。
三个特殊形状仍然先试,因为它们读起来像原文、也让两个前端说同一句话;
通用规则是垫在下面的地板。

`for` 同理:Kernel 的 `for` 已经是拆开的三部分,`{ 声明; while 条件 { 体; 更新 } }`
一条规则全包,**顺带把 `while` 也拿下**。而且 592 个 `for` 里 **405 个其实是
`for-in`**——CFE 早就把它拆成迭代器循环了,轮不到这个编译器认。

### `identical`:问题不是"哪边是 this"

我一开始写的守卫是"必须有一边是 `this`"。错了。真正的问题是
**两边在 Rust 里是不是引用**:一个具体类型的形参是按值传的(翻译出来的值类型是
`Copy`),而一个副本的地址什么也不说明——`identical(this, other)` 会编译,
而且永远返回 false。守卫改成问引用,`&dyn Trait` 的形参才算。

发出的是 `std::ptr::eq(a as *const _ as *const (), …)`:两边的 Rust 类型不同
(`&Self` 和 `&dyn Trait`),而同一性问的是地址,那是两边都有的东西。

### 打通一个之后露出来的三个 bug

1. **`toDouble` 一直是错的。** 那条"值已经是 double 就是空操作"的捷径,
   在接收者是 `int` 时把转换吃掉了。`total + i.toDouble()` 出来是 `total + i`,
   **Dart 里合法,Rust 里不合法**。改成 `as f32`,在 f32 上正是原来假设的空操作。
2. **`String + String` 不是 Rust。** 422 处,一直藏在被拒的方法后面,`for` 一通就露出来。
   发 `format!("{}{}", a, b)`——两端都不用推借用。
3. **两个前端对 `var out = ''` 说的话不一样。** analyzer 只带写出来的类型,
   Kernel 总是知道。以前不可见,因为含 `var` 的方法都因为 `for` 被拒了。
   改成 analyzer 也问 resolution:Rust 未必推得出 Dart 推得出的东西。

### 工具:验证改成并发

用户指出一次起一个 Dart VM 太慢。`fixtures.py` 改成线程池(21 个 fixture 各自
独立目录),**25 秒**;新增 `bin/regen.py` 同样并发地重生成所有 `testdata/src/*.rs`,
**18 秒**。`regen.py` 还固定了两件手工老做错的事:哪个文件该用哪个前端,
以及生成文件需要哪条 `use`——**按内容判断,不靠记忆**。

### 尺子上又一个洞

`// REFUSES:` 检查只看 `// NOT TRANSLATED:` 那一行,而**理由写在下一行**,
于是声明的理由永远匹配不上。这一轮加"必须拒绝的 `identical`"时才撞见。

### 一个诚实的空缺

通用 `Let` **没有单测**。`testdata` 里造不出一个普通的 CFE `Let`
——乱序具名实参不产生,上游那些来自记录模式和 inspector 变换,都是这个编译器
因别的理由拒绝的形状。**普查证明这条路在跑**(1382 条拒绝归零、成员多 1124),
但那只说明它跑了,不说明它算对了。

### 结果

二十一个 fixture 两前端一致,97 个测试全过,五个变异全杀。

---

## 第 28 轮:合成变量、块表达式、`throw` 当值、`unsafeCast`、`break`/`continue`

普查(`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`):

| | 第 27 轮 | 这一轮 |
|---|---|---|
| 零拒绝的类 | 939 | **1043** |
| **发出的成员** | 9845 | **10470(+625)** |
| 拒绝总数 | 5067 | **4441(−626)** |

### 一条改动带掉好几个

上一轮给 CFE 的临时变量起了名字(`_temporaries`)。于是**"合成变量"这条拒绝
本身失去了理由**:当初拒绝它是因为没东西可以称呼它。现在 `_declare` 给它起名,
`VariableGet` 找得回来,它所属的那个 lowering 也就只是周围的几条语句而已。

`BlockExpression` 同理:之前只认级联那一个形状(必须先绑定、必须产出该绑定)。
**块表达式就是"若干语句加一个值"**,那正是 Rust 的块表达式——和上一轮 `Let`
一样,通用规则垫在特殊形状下面。这两条一起,把
`block expression not binding first`(189)和 `cascade binding with no receiver`(243)
一并归零。

`unsafeCast`(437)是 CFE 自己插的、运行时什么也不做的转换,这里也无事可做。

`throw` 当值(151):Rust 没有 throw,也不需要——**`return Err(e)` 的类型是 `!`**,
放在要值的位置上正合适。

### `??` 右边是 `throw` 时,我又踩了同一个坑

`a ?? throw e` 发成了 `unwrap_or_else(|| return Err(..))`——那个 `return`
**从闭包返回**。和 try 体那次一模一样。改发 `match`:没有闭包可逃,
抛的那一臂直接发散。

### `continue` 那个 bug 是靠死循环发现的

我把 CFE 的标签块"还原"成了 Rust 的 `break`/`continue`,理由是 analyzer 那边
看见的是程序员写的词。`break` 对了,**`continue` 错了**:
Dart 的 `continue` 在 `for` 里**会执行更新式**,Rust 的不会——直接死循环,
`cargo test` 挂住不动。CFE 用标签块正是为了这个。

所以规则改成:**有更新式时保留 CFE 的形状**(体是标签块,`continue` 跳出它,
正好落在更新式上);没有更新式(`while`)才还原成 `continue`。
而体一旦成了标签块,里面的 `break` 也必须带标签——Rust 不许无标签 `break`
穿过标签块。两个前端还必须**按同样的顺序分配同样的标签号**,否则文本对不上。

### 变异

四个全杀。有一个我给 `cargo test` 加了超时:把 `continue` 改回错的版本会
**挂住而不是失败**,挂住也算杀掉,而且如实说出来比干等强。

一个存活过一轮的:"循环体带标签时循环也要带标签"——fixture 里
**没有同时含 `break` 和 `continue` 的循环**,而循环标签只在那时才需要。补上就杀掉了。

### 结果

二十二个 fixture 两前端一致,101 个测试全过,四个变异全杀。

---

## 第 29 轮:`switch`,以及旁边那几个库调用

普查(`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`):

| | 第 28 轮 | 这一轮 |
|---|---|---|
| 零拒绝的类 | 1043 | **1094** |
| **发出的成员** | 10470 | **11078(+608)** |
| 拒绝总数 | 4441 | **3834(−607)** |

### `switch` 量完之后是好消息

628 处,**几乎全在 enum 上**,只有 20 处有 `default`,**0 处空 case 落穿**,
`continue L` 只有 1 处。所以 Rust 的 `match` 不是 `switch` 的近似——
**是同一个构造**,连 Dart 只是假设、Rust 会检查的穷尽性都对得上。

真正要决定的是那个 `break`:1179 个 case 体以它结尾,它的意思是"离开 switch",
而 match 的臂**结束就是离开**。所以它被**丢掉**,不是翻译。

**只丢最后那一个。** 中间的 `break` 是"提前离开 switch",match 的臂做不到——
拒绝。这条一开始只在 Kernel 侧做了,analyzer 侧发出了裸 `break;`:
编译不过所以是响亮的,但**该是拒绝**,否则一个成员的错误会拖垮整个文件。

顺带:Dart 3 里 `case Corner.topLeft:` 是 **`SwitchPatternCase`**,常量得从模式里取。
更复杂的模式(带 `when` 的、非常量的)拒绝——Rust 也有模式,但不是这些模式。

### 三个库调用

`max` 372、`clampDouble` 184。Rust 的 `a.max(b)` **浮点整数同一个拼法**
(`f32::max` 是固有方法,其余走 `Ord::max`),`x.clamp(lo,hi)` 同理。

### 撕下来的方法(364)不是便宜活

量出来 **978/1018 的接收者是 `this`**。所以它不是一个独立的特性,
**是"闭包捕获 this"换了副面孔**——留给所有权那一轮,一并解决。

### 我这轮自己造出来的 blocker,当轮修掉

给临时变量起名之后,`assignment to a synthetic variable` 冒出来 270 条。
理由和第 28 轮那条一样已经不成立了:**能起名就能赋值**。修完
拒绝从 4025 再降到 3834。

### 变异:一个"杀掉"是假的

第四个变异删掉了 `max` 的分支,结果**编译器自己编不过**——那什么也没证明。
换成把映射改成 `'max': 'min'`,编得过、只有答案变了,才是真的杀掉。
另两个存活的是老问题:fixture 里没有那个形状(中间的 `break`、`dart:math` 的调用),
补上就杀掉了。

### 结果

二十三个 fixture 两前端一致,104 个测试全过,四个变异全杀。

---

## 第 30 轮:量清楚所有权,顺手做掉六件现成的

普查(`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`):

| | 第 29 轮 | 这一轮 |
|---|---|---|
| 零拒绝的类 | 1094 | **1172** |
| **发出的成员** | 11078 | **11448(+370)** |
| 拒绝总数 | 3834 | **3480(−354)** |

### 这一轮最重要的产出是一份测量,不是代码

队头两项(闭包捕获 `this` 526、撕方法 390)是同一件事。量了**它们去哪儿**:

**闭包捕获 `this`(857 处)**

| | |
|---|---|
| **传给 `setState`** | **181** |
| 绑到 CFE 临时变量(再当实参) | 200 |
| 静态/工厂实参(存进对象) | 131 |
| 构造函数实参(**存进对象**) | 89 |
| `addPostFrameCallback` | 31 |
| `then` / `addListener` / … | 各 10–25 |

**撕方法(`this` 接收者 978 处)**

| | |
|---|---|
| **`addListener` + `removeListener` + `addStatusListener`** | **401** |
| 绑到临时变量 | 207 |
| **写进字段** | 89 |
| 构造函数实参 | 69 |

两条结论都是坏消息,但都很硬:

1. **监听器那 401 处要求闭包有身份**——加进去还要能按同一个去掉。
   **Rust 的闭包没有身份**。这不是所有权模型能解决的,是接口形状的问题。
2. **`setState` 那 181 处在 Rust 里根本不能成立**,而且和"存不存"无关:
   `self.set_state(|| self.x = true)` 里闭包借 `self`、`set_state` 也借 `self`。
   要么闭包把 `this` 变成**参数**(`set_state(|s| s.x = true)`)——可那要改
   `State.setState` 的签名,而它在另一个库里;要么 `Rc<RefCell<>>`。

所以**没有"挑个安全子集就能推进"这回事**。它卡在一个更前面的问题上:
**翻译出来的对象到底该长什么样**。在那个决定做出来之前,这 907 条不该动。
这个仓库自己的手写移植当初是"五个重入点,五种策略",和这份测量吻合。

### 六件现成的

字符串插值(99)→ `format!`;静态方法当值(111)→ 函数名本身(**不捕获任何东西,
所以撕实例方法的所有权问题在这里根本不出现**);`super.字段`(84)→ 扁平化之后
就是本类字段;局部赋值当值(70);局部函数声明(65)→ 绑到局部的闭包;
`lerpDouble`(67)、`pow`(16)。

### 三个顺带修掉的旧错

1. **`Copy` 一直是无条件 derive 的。** 有 `String` 字段的类编不过——响亮,
   但**这条 derive 是编译器自己写的,不该写自己知道是错的东西**。
2. **赋值也可以是表达式**,`f(total = x)` 里的 `total` 一样要 `mut`,
   而 `let mut` 的遍历器只走语句。
3. `format!` 的转义我又用 Python heredoc 写 Dart 字符串写坏了——**老陷阱,第三次**。
   改用 Edit 工具。

### 变异

五个里四个杀掉。存活的一个是**等价变异**:对局部变量来说
`{ x = v; x }` 和"绑定再产出"结果相同,读回来就是同一个值。如实记着。

### 结果

二十四个 fixture 两前端一致,108 个测试全过。

---

## 下一步:两件,一件是欠账

### 1. Kernel 侧的常量实例(`agree.py` 现在红在这上面)

`agree.py` 是红的,理由具体:Kernel 不翻译 `Alignment::TOP_LEFT` 这类常量,
也不翻译它的运算符,于是 testdata 里的手写测试编译不过。普查里这是排前十的
一整族——`const instance missing dx / value / width / days …`,加上
`const instance of CreationLocation with 0 unnamed constructors`(285)。

CFE 把 `const Alignment(-1.0, -1.0)` 求值成一个 `ConstantExpression`,字段
按**类里的声明顺序**摆着;要还原成 `Alignment::new(-1.0, -1.0)`,得把字段
配回构造函数的形参。**"缺字段"这个拒绝理由本身说明现在的配法是按名字猜的**,
先量:这些常量类里,有多少个的字段名和构造函数形参名对不上。

### 2. `try/finally`(73 处)

和 `Result` 是分开的问题。Rust 的答案是 `Drop` 守卫,而 `Drop` 里不能用 `?`
——所以 finally 体里但凡有一次会失败的调用,这条路就走不通。**先量 73 个
finally 体里有多少含会失败的调用**,再决定是做守卫还是整片拒绝。

## 当前队头

**Kernel,`flutter_build/ef21e168…/app.dill`,前缀 `package:flutter/`**
(525 库 / 2743 类 / 1172 个零拒绝的类 / 11448 个成员发出 / 3480 次拒绝)

| 次数 | 要建的东西 |
|---|---|
| 526 + 390 | **闭包捕获 `this` / 撕方法——同一件事,见第 30 轮的测量,等对象表示定下来** |
| 175 | 写另一个对象的字段 |
| 140 | 非 const 静态字段(`LazyLock`) |
| 118 | `MapLiteral` |
| 87 | `ListConstant` |
| 84 | 有函数体的构造函数 |
| 66 | 没有函数体 |
| 58 | `RecordLiteral` |
| 45 | 工厂构造函数 |
| 44 | `await` |

**下一轮**:继续挑现成的(集合字面量要先决定 `List`/`Map` 用什么表示,
那本身是"翻译 dart:core 最小子集"的头一步);所有权那 916 条等对象表示。

**节奏**:一轮攒几个 blocker 一起实现,最后统一验一次;
`fixtures.py` / `regen.py` 都并发跑。

## 九条要记住的

1. **"零拒绝"(781 / 2743)不等于"翻译好了"**:引用其他库的东西仍靠手写桩。
2. **`testdata` 的桩已经吃力**。真正的答案是翻译 `dart:core` / `dart:ui` 的最小子集。
3. **哪把仪器守哪条守卫。** 断言、rustc、前端对照各有盲区。
   第 25 轮又添一处:`fixtures.py` 比对前剥掉注释,而**拒绝就是注释**,
   于是删掉一条拒绝规则没有任何检查会变红。现在 fixture 用 `// REFUSES:` 自己声明。
   第 27 轮发现那条新检查自己也有洞:它只看 `// NOT TRANSLATED:` 那一行,
   而**理由写在下一行**——加一条新尺子之后要拿一个真会被拒的东西试它。
4. **短路语义最容易静默出错**(`??` 错了十七轮)。
5. **fixture 里的值要能分辨错误配对**——已栽五次。
6. **变异存活时,先查我改的那行和测试断言的是不是同一件事。**
   变异**被杀**时同样要查:第 25 轮有一个"杀了但死在 dill 构建失败上",
   报错取行改准了才看清。
7. **探针能分对"是什么"却分错"长什么样"。**
   第 28 轮的近亲:**一条拒绝的理由可能已经不成立了**。"合成变量"当初拒绝
   是因为没东西能称呼它;上一轮给它起了名字之后,那条拒绝就只是没人回头看。
   加了新能力之后,回头查一遍还有哪些拒绝是靠旧前提立着的。
8. **拒绝数升高可能是好事。** 第 9 轮(私有成员)和第 24 轮(构造函数体)
   都是把静默丢弃换成明确拒绝,数字变差而尺子变准。
   **报进度时要说清是哪一种。**
9. **数字必须带量它的条件。** 第 24 轮记的 `10040` 没写 dill 和前缀,
   第 25 轮想比时发现任何组合都复现不出,那一轮的进度记录就此作废。
   同理 `agree.py AGREE` 那句:回去重跑,HEAD 上就是红的,**红了两轮没人看**。
   写下"某某检查是绿的"之前,先真的跑一遍。
