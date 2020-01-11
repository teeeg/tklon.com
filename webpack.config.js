module.exports = {
    entry: {
        tags: "./source/typescripts/tags.ts"
    },
    output: {
        filename: "[name].js",
        path: __dirname + "/.tmp/javascripts/"
    },

    resolve: {
        // Add '.ts' and '.tsx' as resolvable extensions.
        extensions: [".ts", ".js"]
    },

    module: {
        loaders: [
            // All files with a '.ts' extension will be handled by 'awesome-typescript-loader'.
            { test: /\.ts$/, loader: "awesome-typescript-loader" }
        ]
    }
};