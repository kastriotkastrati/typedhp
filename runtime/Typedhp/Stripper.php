<?php declare(strict_types=1);

namespace Typedhp;

use ParseError;
use type Typedhp\Result;

final class Stripper
{
    private const int STRIP_ERROR_EXIT_CODE = 1;

    public static function isVendorPath(string $path): bool {
        return str_contains($path, DIRECTORY_SEPARATOR . 'vendor' . DIRECTORY_SEPARATOR);
    }

    public static function source(string $path, string $original): Result<string, non-empty-string> {
        $home = dirname(__DIR__);
        $binary = "{$home}/bin/typedhp";
        $binaryModifiedAt = filemtime($binary);
        $binarySize = filesize($binary);
        $isBinaryPresent = $binaryModifiedAt !== false && $binarySize !== false;
        if (!$isBinaryPresent) {
            return new Err("typedhp: {$binary} is missing; run `typedhp install` again");
        }

        $sourceHash = hash('xxh128', $original);
        $cachePath = "{$home}/cache/{$binaryModifiedAt}-{$binarySize}-{$sourceHash}.php";
        $isCached = is_file($cachePath);
        if ($isCached) {
            $cached = file_get_contents($cachePath);
            $isCacheRead = $cached !== false;
            return $isCacheRead ? new Ok($cached) : new Err("typedhp: cannot read {$cachePath}");
        }

        $started = ProcessRun::start([$binary, 'strip', '--stdin', $path], $original);
        if (!$started->ok) {
            return $started;
        }

        $run = $started->data;
        $isStripError = $run->exitCode === self::STRIP_ERROR_EXIT_CODE;
        if ($isStripError) {
            return self::parseErrorSource($run->errors);
        }

        $isStripped = $run->exitCode === 0;
        if (!$isStripped) {
            return new Err("typedhp: stripping {$path} failed with exit code {$run->exitCode}: {$run->errors}");
        }

        $stripped = new Ok($run->output);
        $randomSuffix = Result::from(static fn(): string => bin2hex(random_bytes(8)));
        if (!$randomSuffix->ok) {
            return $stripped;
        }

        $temporaryPath = "{$cachePath}.{$randomSuffix->data}.tmp";
        $isWritten = file_put_contents($temporaryPath, $run->output) !== false;
        if ($isWritten) {
            rename($temporaryPath, $cachePath);
        }

        return $stripped;
    }

    private static function parseErrorSource(string $errors): Result<string, non-empty-string> {
        $match = [];
        $isLocated = preg_match('/:(\d+): (.+)\z/s', rtrim($errors), $match) === 1;
        $line = $isLocated ? (int) $match[1] : 0;
        $isLineValid = $line >= 1;
        if (!$isLineValid) {
            return new Err("typedhp: unexpected error output: {$errors}");
        }

        $message = var_export("typedhp: {$match[2]}", return: true);
        $lineBreaks = str_repeat("\n", $line - 1);
        $source = sprintf('<?php %sthrow new \\%s(%s);', $lineBreaks, ParseError::class, $message);
        return new Ok($source);
    }
}
