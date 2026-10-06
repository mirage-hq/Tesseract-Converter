// AE random API approximation. The FNV/xorshift stream, Gaussian transform and
// coherent lattice kernel below are our own implementation, NOT Adobe's
// random sequence/kernel.
function __aeRandom(identity, clock, authoredValue, mark, scalar, finite, fail) {
    function hash(text) {
        let h = 2166136261;
        for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
        return h >>> 0;
    }
    // FNV-1a alone maps inputs that differ only in their last character to
    // values a fixed multiple apart, so adjacent lattice cells and frames were
    // almost equal. A murmur3 finalizer decorrelates them.
    function mix(h) {
        h ^= h >>> 16; h = Math.imul(h, 0x85ebca6b);
        h ^= h >>> 13; h = Math.imul(h, 0xc2b2ae35);
        h ^= h >>> 16;
        return h >>> 0;
    }
    let offset = 0, timeless = false, state, streamClock;
    function reset() {
        streamClock = clock();
        state = mix(hash(identity + ':' + offset + ':' + (timeless ? 'timeless' : streamClock))) || 1;
    }
    reset();
    function unit() {
        if (!timeless && streamClock !== clock()) reset();
        state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
        return (state >>> 0) / 4294967296;
    }
    function seedRandom(seed, freeze = false) {
        mark('seedRandom');
        offset = scalar(seed);
        if (typeof freeze !== 'boolean') fail('seedRandom timeless must be boolean');
        timeless = freeze;
        reset();
    }
    function bounds(args) {
        if (args.length > 2) fail('random bounds require zero, one or two arguments');
        let low = args.length === 2 ? finite(args[0]) : 0;
        let high = args.length === 0 ? 1 : finite(args[args.length - 1]);
        if (Array.isArray(low) || Array.isArray(high)) {
            const size = Array.isArray(low) ? low.length : high.length;
            if (!Array.isArray(low)) low = Array(size).fill(low);
            if (!Array.isArray(high)) high = Array(size).fill(high);
            if (low.length !== high.length) fail('random vector bounds have different dimensions');
        }
        return [low, high];
    }
    function ranged(args, draw) {
        const [low, high] = bounds(args);
        return finite(Array.isArray(low)
            ? low.map((v, i) => v + (high[i] - v) * draw())
            : low + (high - low) * draw());
    }
    function random(...args) {
        mark('random');
        return ranged(args, unit);
    }
    function gaussRandom(...args) {
        mark('gaussRandom');
        // Box-Muller; range/6 standard deviation, with deliberately unbounded tails.
        return ranged(args, () => 0.5 + Math.sqrt(-2 * Math.log(1 - unit()))
            * Math.cos(2 * Math.PI * unit()) / 6);
    }
    function lattice(coordinates, channel) {
        let h = hash(identity + ':' + offset + ':' + channel);
        for (const x of coordinates) h = hash(h + ':' + x);
        return mix(h) / 2147483648 - 1;
    }
    function coherent(coordinates, channel) {
        const origins = coordinates.map(x => {
            x = scalar(x);
            if (!Number.isSafeInteger(Math.floor(x)) || !Number.isSafeInteger(Math.floor(x) + 1))
                fail('random lattice coordinate exceeds exact integer range');
            return Math.floor(x);
        });
        const weights = coordinates.map((x, i) => {
            const f = x - origins[i];
            return f * f * (3 - 2 * f);
        });
        let result = 0;
        for (let corner = 0; corner < (1 << coordinates.length); corner++) {
            let weight = 1;
            const point = origins.map((x, i) => {
                const side = (corner >> i) & 1;
                weight *= side ? weights[i] : 1 - weights[i];
                return x + side;
            });
            result += lattice(point, channel) * weight;
        }
        return result;
    }
    function noise(value) {
        mark('noise');
        value = finite(value);
        const coordinates = Array.isArray(value) ? value : [value];
        if (coordinates.length > 3) fail('noise supports one, two or three coordinates');
        return finite(coherent(coordinates, 'noise'));
    }
    function wiggle(frequency, amplitude, octaves = 1, multiplier = 0.5, t = clock()) {
        mark('wiggle');
        frequency = scalar(frequency); amplitude = scalar(amplitude);
        octaves = scalar(octaves); multiplier = scalar(multiplier); t = scalar(t);
        if (frequency < 0 || !Number.isInteger(octaves) || octaves < 1 || octaves > 16)
            fail('wiggle requires nonnegative frequency and 1..16 integer octaves');
        const value = finite(authoredValue(t));
        const vector = Array.isArray(value) ? value : [value];
        const result = vector.map((v, component) => {
            if (frequency === 0) return v;
            let strength = amplitude, hz = frequency, perturbation = 0;
            for (let octave = 0; octave < octaves; octave++) {
                perturbation += strength * coherent([t * hz], 'wiggle:' + component + ':' + octave);
                hz *= 2; strength *= multiplier;
            }
            return v + perturbation;
        });
        return finite(Array.isArray(value) ? result : result[0]);
    }
    return {seedRandom, random, gaussRandom, wiggle, noise};
}
