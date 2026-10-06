<?php declare(strict_types=1);

namespace Typedhp;

use ParseError;
use RuntimeException;

final class Stripper
{
    private const int STRIP_ERROR_EXIT_CODE = 1;

    public static function isVendorPath(string $path): bool {
        return str_contains($path, DIRECTORY_SEPARATOR . 'vendor' . DIRECTORY_SEPARATOR);
    }

    public static function source(string $path, string $original): string {
        $home = dirname(__DIR__);
        $binary = "{$home}/bin/typedhp";
        $binaryModifiedAt = filemtime($binary);
        $binarySize = filesize($binary);
        $isBinaryPresent = $binaryModifiedAt !== false && $binarySize !== false;
        if (!$isBinaryPresent) {
            throw new RuntimeException("typedhp: {$binary} is missing; run `typedhp install` again");
        }

        $sourceHash = hash('xxh128', $original);
        $cachePath = "{$home}/cache/{$binaryModifiedAt}-{$binarySize}-{$sourceHash}.php";
        $isCached = is_file($cachePath);
        if ($isCached) {
            $cached = file_get_contents($cachePath);
            $isCacheRead = $cached !== false;
            return $isCacheRead ? $cached : throw new RuntimeException("typedhp: cannot read {$cachePath}");
        }

        $run = ProcessRun::start([$binary, 'strip', '--stdin', $path], $original);
        $isStripError = $run->exitCode === self::STRIP_ERROR_EXIT_CODE;
        if ($isStripError) {
            return self::parseErrorSource($run->errors);
        }

        $isStripped = $run->exitCode === 0;
        if (!$isStripped) {
            throw new RuntimeException(
                "typedhp: stripping {$path} failed with exit code {$run->exitCode}: {$run->errors}",
            );
        }

        $temporaryPath = sprintf('%s.%s.tmp', $cachePath, bin2hex(random_bytes(8)));
        $isWritten = file_put_contents($temporaryPath, $run->output) !== false;
        if ($isWritten) {
            rename($temporaryPath, $cachePath);
        }

        return $run->output;
    }

    private static function parseErrorSource(string $errors): string {
        $match = [];
        $isLocated = preg_match('/:(\d+): (.+)\z/s', rtrim($errors), $match) === 1;
        $line = $isLocated ? (int) $match[1] : 0;
        $isLineValid = $line >= 1;
        if (!$isLineValid) {
            throw new RuntimeException("typedhp: unexpected error output: {$errors}");
        }

        $message = var_export("typedhp: {$match[2]}", return: true);
        return sprintf('<?php %sthrow new \\%s(%s);', str_repeat("\n", $line - 1), ParseError::class, $message);
    }
}
