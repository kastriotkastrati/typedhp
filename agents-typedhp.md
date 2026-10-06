# typedhp for agents

This project writes PHP with typedhp: TypeScript-style types, written inline in ordinary `.php` files. PHP can't parse these types on its own. typedhp strips them before PHP runs a file, and checks them with Mago. Read this before you write or change PHP here.

## Commands

| To | Run |
|---|---|
| run a script | `php --strip-types script.php` |
| run artisan | `php --strip-types artisan <command>` |
| run the tests | `php --strip-types artisan test` or `php --strip-types vendor/bin/phpunit` |
| check every file | `php --typecheck`, from the project root |
| check some files | `php --typecheck src/Users.php` (extra arguments go to `mago analyze`) |
| see what PHP runs | `typedhp strip src/Users.php` |
| see what Mago checked | open `.typedhp/check/src/Users.php` |

`php --typecheck` prints one line per problem, `path:line: level[code]: message`, then `typedhp: found N issues` or `typedhp: no issues found`. It exits with 1 when it finds an error. `error[typedhp]` means typedhp could not read a type; the other codes come from Mago.

Don't:

- **Run typed files with plain `php`.** It stops at the first type it doesn't know with a parse error. Always pass `--strip-types`.
- **Run `php -l`, `mago fmt` or `mago lint` on typed files.** They can't parse typed syntax. Expect the same from any tool that parses PHP itself, such as a formatter or linter.
- **Put typed code under `vendor/`.** typedhp never strips files there.

## Where types go

Write types inline, wherever PHP takes a type: parameters, return types, properties, promoted constructor parameters, class constants, closures and arrow functions.

```php
final class Settings
{
    public const list<string> NAMES = ['debug', 'cache'];

    public ?array{debug: bool} $flags = null;

    public function __construct(private positive-int $ttl) {}

    public function label(non-empty-string $prefix): non-empty-string {
        $format = fn(positive-int $value): string => "{$prefix}: {$value}";
        return $format($this->ttl);
    }
}
```

Don't write `@param`, `@return`, `@var`, `@template`, `@extends`, `@implements` or `@use` docblocks. typedhp writes them for Mago from your inline types. Mago reads a docblock you write by hand as it is, so a type alias inside it is an unknown class.

## Types

Plain PHP types work as before. On top of them, use these. The right column is what PHP checks at runtime; Mago checks the whole type.

| Type | Means | PHP runs it as |
|---|---|---|
| `list<T>`, `non-empty-list<T>` | array with keys 0, 1, 2, … | `array` |
| `array<K, V>`, `non-empty-array<K, V>` | array with these keys and values | `array` |
| `array{id: int, name?: string}` | array with these keys; `?` marks a key that may be missing | `array` |
| `object{id: int}` | object with these properties | `object` |
| `iterable<K, V>` | | `iterable` |
| `\Generator<K, V, TSend, TReturn>`, `\Traversable<K, V>`, any generic class | | the class |
| `positive-int`, `negative-int`, `non-negative-int` | | `int` |
| `int<1, 10>`, `int<0, max>` | int in this range | `int` |
| `non-empty-string`, `numeric-string`, `lowercase-string`, `literal-string`, `callable-string` | | `string` |
| `class-string`, `class-string<\Throwable>` | name of a class (that extends `\Throwable`) | `string` |
| `'asc'\|'desc'` | one of these strings | `string` |
| `1\|2\|3` | one of these ints | `int` |
| `array-key` | | `int\|string` |
| `numeric` | | `int\|float\|string` |
| `scalar` | | `int\|float\|string\|bool` |
| `callable(int): string` | | `callable` |
| `\Closure(int): string` | | `\Closure` |
| `key-of<T>`, `value-of<T>` | | `mixed` |
| a type parameter `T` | | its bound, or `mixed` |
| a type alias | | `mixed` |

`?` and `|null` keep working: `?list<int>` runs as `?array`.

PHP checks only the right column. A `positive-int` parameter still accepts `0` when PHP runs it, and an alias accepts anything. Validate data from outside the program, such as request input, JSON and database rows, with real runtime checks.

## Generics

Functions, methods, classes, interfaces and traits declare type parameters in `<...>` after their name. Closures declare them after `fn` or `function`.

| Write | Means |
|---|---|
| `<T>` | any type |
| `<T: \Countable>` | `T` must be a `\Countable`; PHP runs `T` as `\Countable` |
| `<+T>` | covariant: `Collection<Dog>` is a `Collection<Animal>`. Only for types that hand `T` out and never take it in |
| `<-T>` | contravariant: a `Handler<Animal>` is a `Handler<Dog>`. Only for types that take `T` in and never hand it out |
| `<T = int>` | a default; Mago ignores it |
| `<K, V>` | several parameters |

Pass type arguments after a class name, wherever it appears: `extends Pair<int, int>`, `implements Repository<User>`, `use RemembersLast<User>`, and in types such as `Collection<User>`.

```php
final readonly class Collection<+T> implements \IteratorAggregate<int, T>, \Countable
{
    public function __construct(private list<T> $items = []) {}

    public function map<U>(\Closure(T): U $mapper): Collection<U> {
        return new Collection(array_map($mapper, $this->items));
    }

    public function first(): ?T {
        return $this->items[0] ?? null;
    }

    public function count(): int {
        return count($this->items);
    }

    public function getIterator(): \ArrayIterator<int, T> {
        return new \ArrayIterator($this->items);
    }
}

interface Repository<T>
{
    public function find(UserId $id): ?T;

    public function all(): Collection<T>;
}

trait RemembersLast<T>
{
    private ?T $last = null;

    public function remember(T $value): void {
        $this->last = $value;
    }
}

final class UserRepository implements Repository<User>
{
    use RemembersLast<User>;

    // ...
}

interface Handler<-T>
{
    public function handle(T $value): void;
}

function first<T>(list<T> $items): ?T {
    return $items[0] ?? null;
}

function longest<T: \Countable>(T $a, T $b): T {
    return count($a) >= count($b) ? $a : $b;
}

$identity = fn<T>(T $value): T => $value;
```

Rules:

- **Let Mago infer type arguments.** Write `first([3, 4])` and `new Collection($users)`, not `first::<int>(...)`.
- **Use `::<...>` only to create an empty generic object:** `$ids = new Collection::<int>();`. typedhp turns this into `/** @var Collection<int> $ids */`, and Mago trusts that without checking. So with arguments, `$box = new Collection::<string>([1, 2])` passes the check although it's wrong. Anywhere else, such as `first::<string>(...)` or `$repository->load::<User>()`, typedhp drops the type arguments and they do nothing.
- **Match variance.** Mago rejects `class Box<+T> implements Repository<T>` when `Repository` declares an invariant `<T>`.
- **A child class doesn't get its constructor checked.** After `class Range extends Pair<int, int>`, Mago 1.51 does not check `new Range('a', 'b')` against `int`. This is a gap in Mago. Give the child its own typed constructor when its arguments matter.

## Type aliases

```php
<?php // src/types.php

namespace App;

type UserId = positive-int;
type Email = non-empty-string;
type User = array{id: UserId, email: Email, admin?: bool};
```

```php
<?php // src/Handler.php

namespace App;

use type App\User;
use type App\UserId as Id;

function lookup(UserRepository $users, Id $id): ?User {
    return $users->find($id);
}
```

- **Declare aliases at the top level of a file,** after `namespace`. An alias can use other aliases.
- **Import an alias with `use type` in every file except the one that declares it, even in the same namespace.** Group imports work: `use type App\{User, UserId};`. Without an import, typedhp reads `UserId` as a class name. PHP then throws a `TypeError` on every call, and Mago reports ``Cannot find class, interface, enum, or type alias `App\UserId` ``. A full name like `\App\UserId` doesn't work either.
- **Each alias has one full name in the whole project.** Declaring `App\UserId` twice is an error.
- **Aliases don't exist at runtime.** PHP runs them as `mixed`, and no file has to be loaded for them.

## What errors look like

```php
function mistakes(UserRepository $users, Mailer $mailer): void {
    $users->find(0);
    $mailer->handle(['id' => 1]);
    longest(1, 2);
}
```

```
$ php --typecheck
typedhp: running mago analyze in .typedhp/check
src/mistakes.php:6: error[invalid-argument]: Invalid argument type for argument #1 of `App\UserRepository::find`: expected `positive-int`, but found `int(0)`.
src/mistakes.php:7: error[possibly-invalid-argument]: Possible argument type mismatch for argument #1 of `App\Mailer::handle`: expected `array{'admin'?: bool, 'email': non-empty-string, 'id': positive-int}`, but possibly received `array{'id': int(1)}`.
src/mistakes.php:8: error[template-constraint-violation]: Argument type mismatch for template `T`.
src/mistakes.php:8: error[template-constraint-violation]: Argument type mismatch for template `T`.
src/mistakes.php:8: error[invalid-argument]: Invalid argument type for argument #1 of `App\longest`: expected `('T.app\longest() extends Countable)`, but found `int(1)`.
src/mistakes.php:8: error[invalid-argument]: Invalid argument type for argument #2 of `App\longest`: expected `('T.app\longest() extends Countable)`, but found `int(2)`.
typedhp: found 6 issues
```

Line numbers point at your file. typedhp never moves a line, so errors from PHP at runtime point at the right line too.

## Laravel

- The container injects typed classes as before. A constructor parameter `Stack<string> $names` receives a `Stack`.
- In `routes/console.php`, Mago doesn't know what `$this` is inside a command closure. Start the closure with `/** @var \Illuminate\Foundation\Console\ClosureCommand $this */`.
- `$this->argument('name')` can return `null` or an array. Narrow it, for example with `is_string()`, before you pass it to a typed parameter.

## Before you finish

1. `php --typecheck` prints `typedhp: no issues found`.
2. The tests pass through `php --strip-types`.
3. You wrote types inline, not in docblocks, and imported each alias you used with `use type`.
