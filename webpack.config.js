const path = require('path');

module.exports = {
    entry: {
        tags: "./source/typescripts/tags.ts"
    },
    module: {
        rules: [
            {
                test: /\.ts$/,
                use: 'ts-loader',
                exclude: /node_modules/,
            },
        ],
    },

    resolve: {
        extensions: [".ts", ".js"]
    },

    output: {
        filename: '[name].js',
        path: path.resolve(__dirname, '.tmp/javascripts/'),
    },
};