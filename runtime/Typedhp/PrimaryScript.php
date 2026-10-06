<?php declare(strict_types=1);

namespace Typedhp;

use type Typedhp\Result;

final class PrimaryScript
{
    public static function typedPath(): Result<?string, non-empty-string> {
        $isCommandLine = PHP_SAPI === 'cli';
        if (!$isCommandLine) {
            return new Ok(null);
        }

        $scriptFilename = $_SERVER['SCRIPT_FILENAME'];
        $isScriptFile = is_file($scriptFilename);
        if (!$isScriptFile) {
            return new Ok(null);
        }

        $path = realpath($scriptFilename);
        $original = file_get_contents($scriptFilename);
        $isStrippable = $path !== false && $original !== false && !Stripper::isVendorPath($path);
        if (!$isStrippable) {
            return new Ok(null);
        }

        $stripped = Stripper::source($path, $original);
        if (!$stripped->ok) {
            return $stripped;
        }

        $isTyped = $stripped->data !== $original;
        $typedPath = $isTyped ? $path : null;
        return new Ok($typedPath);
    }
}
