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

## 当前队头(全 framework:685 文件 / 1931 个公开类 / 7012 次拒绝)

| 次数 | 要建的东西 |
|---|---|
| 692 | super 调用 |
| 499 | setter |
| 367 | 后缀 `!` |
| 285 | 赋值表达式 |
| 212 | super 构造函数调用 |
| 209 | 调用一个函数值(闭包) |
| 197 | 字符串插值 |
| 181 | `is` |
| 177 | 枚举值引用 |
| 138 | 闭包字面量 |

**下一步**:`super` 调用(692,新队头)。它是本轮的直接延续——
trait 有了默认方法,`super.add(other)` 就该降成
`Base::add(self, other)`(显式调 trait 的默认实现)。
这是 Rust 现成就有的东西,和本轮"两种语言在这里本来就一样"是同一条脉络。

之后 `setter`(499)+ 赋值(285)= 784,是**同一件事**:
可变性。需要 `&mut self` 和一个"什么时候可变"的模型,
比 super 大,应该单独规划。

**关于"零拒绝"这个数要留一句警告**:它现在是 438,但它**系统性偏乐观**。
私有成员是**跳过**不是拒绝,而 Flutter 的实现几乎全在私有 State 类里,
所以一个 widget 报"零拒绝"往往意味着"它的实质部分根本没被看"。
`CupertinoApp` 就是这样:零拒绝,却生成了
`Option<Route<dynamic>? Function(RouteSettings)?>` 这种根本不是 Rust 的东西。

用 `--emit-dir` 可以把零拒绝的类写出来再用 rustfmt 验解析。
上次全量测的结果:**347 个零拒绝里,254 个能解析,235 个不含未翻译的 Dart 类型,
而真正验证过能编译的只有 2 个**(`Alignment`、`EdgeInsets`,还得手写桩)。
**报进度时用后面这些数,不要用"零拒绝"。**
