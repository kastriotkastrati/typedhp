<?php declare(strict_types=1);

namespace Typedhp;

use Closure;
use RuntimeException;

/**
 * Implements PHP's stream wrapper protocol, which fixes its method names and count:
 * https://www.php.net/manual/en/class.streamwrapper.php
 */
// @mago-expect lint:too-many-methods,cyclomatic-complexity
final class StrippingFileWrapper
{
    private const int STREAM_OPEN_FOR_INCLUDE = 0x80;

    private static string $primaryScript = '';

    public ?resource $context = null;

    private resource $handle;

    private resource $directory;

    private ?array<int|string, int> $strippedStat = null;

    public static function enable(): bool {
        stream_wrapper_unregister('file');
        stream_wrapper_register('file', self::class);
        $typedPrimaryScript = self::outsideWrapper(self::typedPrimaryScript(...));
        $runsPrimaryScript = $typedPrimaryScript !== null;
        if ($runsPrimaryScript) {
            self::$primaryScript = $typedPrimaryScript;
        }

        return $runsPrimaryScript;
    }

    public static function primaryScript(): string {
        return self::$primaryScript;
    }

    public function stream_open(string $path, string $mode, int $options, ?string &$opened_path): bool {
        $isInclude = ($options & self::STREAM_OPEN_FOR_INCLUDE) !== 0;
        $stripsFile = $isInclude && !Stripper::isVendorPath($path);
        if ($stripsFile) {
            return self::outsideWrapper(fn(): bool => $this->openStripped($path));
        }

        $usesIncludePath = ($options & STREAM_USE_PATH) !== 0;
        $handle = self::outsideWrapper(fn() => fopen($path, $mode, $usesIncludePath, $this->context));
        $isOpen = $handle !== false;
        if ($isOpen) {
            $this->handle = $handle;
        }

        if ($isOpen && $usesIncludePath) {
            $opened_path = stream_get_meta_data($handle)['uri'];
        }

        return $isOpen;
    }

    public function stream_read(int $count): string|false {
        return fread($this->handle, $count);
    }

    public function stream_write(string $data): int {
        $written = fwrite($this->handle, $data);
        $isWritten = $written !== false;
        return $isWritten ? $written : 0;
    }

    public function stream_eof(): bool {
        return feof($this->handle);
    }

    public function stream_tell(): int {
        $position = ftell($this->handle);
        $isKnown = $position !== false;
        return $isKnown ? $position : throw new RuntimeException('typedhp: cannot read the stream position');
    }

    public function stream_seek(int $offset, int $whence): bool {
        return fseek($this->handle, $offset, $whence) === 0;
    }

    public function stream_flush(): bool {
        return fflush($this->handle);
    }

    public function stream_truncate(int $new_size): bool {
        return ftruncate($this->handle, $new_size);
    }

    public function stream_lock(int $operation): bool {
        $isSupportCheck = $operation === 0;
        if ($isSupportCheck) {
            return true;
        }

        return flock($this->handle, $operation);
    }

    public function stream_set_option(int $option, int $arg1, ?int $arg2): bool {
        return match ($option) {
            STREAM_OPTION_BLOCKING => stream_set_blocking($this->handle, $arg1 === 1),
            STREAM_OPTION_READ_TIMEOUT => stream_set_timeout($this->handle, $arg1, (int) $arg2),
            STREAM_OPTION_WRITE_BUFFER => stream_set_write_buffer($this->handle, (int) $arg2) === 0,
            STREAM_OPTION_READ_BUFFER => stream_set_read_buffer($this->handle, (int) $arg2) === 0,
            default => false,
        };
    }

    public function stream_stat(): array<int|string, int>|false {
        $isStrippedInclude = $this->strippedStat !== null;
        if ($isStrippedInclude) {
            return $this->strippedStat;
        }

        return fstat($this->handle);
    }

    public function stream_cast(int $cast_as): resource {
        return $this->handle;
    }

    public function stream_close(): void {
        fclose($this->handle);
    }

    public function stream_metadata(string $path, int $option, mixed $value): bool {
        return self::outsideWrapper(static fn(): bool => match ($option) {
            STREAM_META_TOUCH => is_array($value) && touch($path, ...array_filter($value, is_int(...))),
            STREAM_META_OWNER, STREAM_META_OWNER_NAME => (is_int($value) || is_string($value)) && chown($path, $value),
            STREAM_META_GROUP, STREAM_META_GROUP_NAME => (is_int($value) || is_string($value)) && chgrp($path, $value),
            STREAM_META_ACCESS => is_int($value) && chmod($path, $value),
            default => false,
        });
    }

    public function url_stat(string $path, int $flags): array<int|string, int>|false {
        $readsLink = ($flags & STREAM_URL_STAT_LINK) !== 0;
        $isQuiet = ($flags & STREAM_URL_STAT_QUIET) !== 0;
        return self::outsideWrapper(static function () use ($path, $readsLink, $isQuiet): array|false {
            $exists = $readsLink ? is_link($path) || file_exists($path) : file_exists($path);
            $skipsWarning = !$exists && $isQuiet;
            if ($skipsWarning) {
                return false;
            }

            return $readsLink ? lstat($path) : stat($path);
        });
    }

    public function unlink(string $path): bool {
        return self::outsideWrapper(fn(): bool => unlink($path, $this->context));
    }

    public function rename(string $path_from, string $path_to): bool {
        return self::outsideWrapper(fn(): bool => rename($path_from, $path_to, $this->context));
    }

    public function mkdir(string $path, int $mode, int $options): bool {
        $isRecursive = ($options & STREAM_MKDIR_RECURSIVE) !== 0;
        return self::outsideWrapper(fn(): bool => mkdir($path, $mode, $isRecursive, $this->context));
    }

    public function rmdir(string $path, int $options): bool {
        return self::outsideWrapper(fn(): bool => rmdir($path, $this->context));
    }

    public function dir_opendir(string $path, int $options): bool {
        $directory = self::outsideWrapper(fn() => opendir($path, $this->context));
        $isOpen = $directory !== false;
        if ($isOpen) {
            $this->directory = $directory;
        }

        return $isOpen;
    }

    public function dir_readdir(): string|false {
        return readdir($this->directory);
    }

    public function dir_rewinddir(): bool {
        rewinddir($this->directory);
        return true;
    }

    public function dir_closedir(): bool {
        closedir($this->directory);
        return true;
    }

    private function openStripped(string $path): bool {
        $isFile = is_file($path);
        if (!$isFile) {
            return false;
        }

        $original = file_get_contents($path);
        $stat = stat($path);
        $isRead = $original !== false && $stat !== false;
        if (!$isRead) {
            return false;
        }

        $source = Stripper::source($path, $original);
        $memory = fopen('php://memory', mode: 'w+b');
        fwrite($memory, $source);
        rewind($memory);
        $size = strlen($source);
        $this->handle = $memory;
        $this->strippedStat = array_replace($stat, ['size' => $size, 7 => $size]);
        return true;
    }

    private static function typedPrimaryScript(): ?string {
        $isCommandLine = PHP_SAPI === 'cli';
        if (!$isCommandLine) {
            return null;
        }

        $scriptFilename = $_SERVER['SCRIPT_FILENAME'];
        $isScriptFile = is_file($scriptFilename);
        if (!$isScriptFile) {
            return null;
        }

        $path = realpath($scriptFilename);
        $original = file_get_contents($scriptFilename);
        $isStrippable = $path !== false && $original !== false && !Stripper::isVendorPath($path);
        if (!$isStrippable) {
            return null;
        }

        $isTyped = Stripper::source($path, $original) !== $original;
        return $isTyped ? $path : null;
    }

    private static function outsideWrapper<T>(Closure(): T $operation): T {
        stream_wrapper_restore('file');
        try {
            return $operation();
        } finally {
            stream_wrapper_unregister('file');
            stream_wrapper_register('file', self::class);
        }
    }
}
