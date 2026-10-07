# typedhp for agents

This project writes PHP with typedhp: TypeScript-style types, written inline in ordinary `.php` files. PHP can't parse these types on its own. typedhp strips them before PHP runs a file, and checks them with Mago. Read this before you write or change PHP here.

## Commands

| To | Run |
|---|---|
| run a script | `php --strip-types script.php` |
| run artisan | `php --strip-types artisan <command>` |
| run the tests | `php --strip-types artisan test` or `php --strip-types vendor/bin/phpunit` |
| check every file | `php --typecheck` or `mago analyze`, from the project root |
| check some files | `php --typecheck src/Users.php` (extra arguments go to `mago analyze`) |
| format files | `mago format`, or `mago format src/Users.php` |
| lint files | `mago lint`, or `mago lint src/Users.php` |
| fix lint issues | `mago lint --fix`, then `mago format` |
| see what PHP runs | `typedhp strip src/Users.php` |
| see what Mago checked | open `.typedhp/check/src/Users.php` |

`php --typecheck` prints one line per problem, `path:line: level[code]: message`, then `typedhp: found N issues` or `typedhp: no issues found`. It exits with 1 when it finds an error. `error[typedhp]` means typedhp could not read a type; the other codes come from Mago.

Don't:

- **Run typed files with plain `php`.** It stops at the first type it doesn't know with a parse error. Always pass `--strip-types`.
- **Run `php -l`, `vendor/bin/mago` or other PHP tools on typed files.** They can't parse typed syntax. Run Mago as `mago`, which finds typedhp's shim, so that format, lint and analyze work.
- **Pass `--dry-run` or `--format-after-fix`.** They refuse to run in a project with typed files. Run `mago lint --fix`, then `mago format`. Fix by hand any issue that `mago lint --fix` skips with `error[typedhp]`.
- **Put typed code under `vendor/`.** typedhp never strips files there.

## Where types go

Write types inline, wherever PHP takes a type: parameters, return types, properties, promoted constructor parameters, class constants, closures and arrow functions.

```php
final class Settings
{
    public const list<string> NAMES = ['debug', 'cache'];

    public ?array{debug: bool} $flags = null;

    public function __construct(private PositiveInt $ttl) {}

    public function label(NonEmptyString $prefix): NonEmptyString {
        $format = fn(PositiveInt $value): string => "{$prefix}: {$value}";
        return $format($this->ttl);
    }
}
```

Don't write `@param`, `@return`, `@var`, `@template`, `@extends`, `@implements` or `@use` docblocks. typedhp writes them for Mago from your inline types. Mago reads a docblock you write by hand as it is, so a type alias inside it is an unknown class.

## Types

Plain PHP types work as before. On top of them, use these. The right column is what PHP checks at runtime; Mago checks the whole type.

Write the PHPDoc names that have hyphens in TitleCase: `PositiveInt`, not `positive-int`. typedhp refuses the hyphenated names. Mago's messages show the TitleCase names too.

| Type | Means | PHP runs it as |
|---|---|---|
| `list<T>`, `NonEmptyList<T>` | array with keys 0, 1, 2, … | `array` |
| `array<K, V>`, `NonEmptyArray<K, V>` | array with these keys and values | `array` |
| `array{id: int, name?: string}` | array with these keys; `?` marks a key that may be missing | `array` |
| `object{id: int}` | object with these properties | `object` |
| `iterable<K, V>` | | `iterable` |
| `\Generator<K, V, TSend, TReturn>`, `\Traversable<K, V>`, any generic class | | the class |
| `PositiveInt`, `NegativeInt`, `NonNegativeInt` | | `int` |
| `int<1, 10>`, `int<0, max>` | int in this range | `int` |
| `NonEmptyString`, `NumericString`, `LowercaseString`, `LiteralString`, `CallableString` | | `string` |
| `ClassString`, `ClassString<\Throwable>` | name of a class (that extends `\Throwable`) | `string` |
| `'asc'\|'desc'` | one of these strings | `string` |
| `1\|2\|3` | one of these ints | `int` |
| `ArrayKey` | | `int\|string` |
| `numeric` | | `int\|float\|string` |
| `scalar` | | `int\|float\|string\|bool` |
| `callable(int): string` | | `callable` |
| `\Closure(int): string` | | `\Closure` |
| `KeyOf<T>`, `ValueOf<T>` | | `mixed` |
| a type parameter `T` | | its bound, or `mixed` |
| a type alias | | `mixed` |

`?` and `|null` keep working: `?list<int>` runs as `?array`.

PHP checks only the right column. A `PositiveInt` parameter still accepts `0` when PHP runs it, and an alias accepts anything. Validate data from outside the program, such as request input, JSON and database rows, with real runtime checks.

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

type UserId = PositiveInt;
type Email = NonEmptyString;
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
- **Aliases can take type parameters, with defaults:** `type Result<T, E = string> = Ok<T>|Err<E>;`. Pass arguments as you would to a generic class: `Result<User>`, `Result<User, \Throwable>`. Too many arguments, or a missing one without a default, is an error. Alias parameters can't have a bound or a variance.
- **Narrow a union on a property with a fixed value.** When `Ok` has `public true $ok` and `Err` has `public false $ok`, Mago reads `if (!$result->ok) { return $result; }` and then knows `$result->data` is the `Ok` value.

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
src/mistakes.php:6: error[invalid-argument]: Invalid argument type for argument #1 of `App\UserRepository::find`: expected `PositiveInt`, but found `int(0)`.
src/mistakes.php:7: error[possibly-invalid-argument]: Possible argument type mismatch for argument #1 of `App\Mailer::handle`: expected `array{'admin'?: bool, 'email': NonEmptyString, 'id': PositiveInt}`, but possibly received `array{'id': int(1)}`.
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
2. You ran `mago format`, and `mago lint` prints `typedhp: no issues found`.
3. The tests pass through `php --strip-types`.
4. You wrote types inline, not in docblocks, and imported each alias you used with `use type`.
