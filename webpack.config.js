const path = require('path');
const CopyPlugin = require('copy-webpack-plugin');
const WasmPackPlugin = require('@wasm-tool/wasm-pack-plugin');
const MiniCssExtractPlugin = require('mini-css-extract-plugin');
const VueLoaderPlugin = require('vue-loader/lib/plugin');

const dist = path.resolve(__dirname, 'dist');
const static = path.resolve(__dirname, 'static');

module.exports = env => {
  return {
    entry: {
      main: env.wc ?
        ['./js/main-wc.js', './styles/main.styl'] :
        ['./js/main-app.js', './styles/main.styl']
    },
    devtool: '#source-map',
    output: {
      path: dist,
      filename: '[name].js'
    },
    devServer: {
      openPage: 'dev.html',
      // contentBase: [static, path.resolve(__dirname, 'js/components')],
      contentBase: static,
      watchContentBase: true,
      overlay: true,
      hot: true,
    },
    plugins: [
      new MiniCssExtractPlugin({
        // filename: 'style.css'
      }),
      new VueLoaderPlugin(),
      new CopyPlugin([
        './package.json',
        static + '/index.html',
        path.resolve(__dirname, 'electron')
      ]),
      new WasmPackPlugin({
        crateDirectory: path.resolve(__dirname, 'rust'),
        watchDirectories: [
          path.resolve(__dirname, 'rust/lib/shapex/src/'),
          path.resolve(__dirname, 'rust/lib/alchemy-core/src/'),
        ],
        extraArgs: '--out-name wasm_index',
      }),
    ],
    module: {
      rules: [
        {
          test: /\.html$/i,
          use: 'raw-loader',
        },
        {
          test: /\.vue$/,
          loader: 'vue-loader',
          exclude: path.resolve(__dirname, 'node_modules'),
          options: {
            shadowMode: env.wc
          }
        },
        {
          test: /\.pug$/,
          loader: 'pug-plain-loader'
        },
        {
          test: /\.styl$/,
          use: [
            { loader: MiniCssExtractPlugin.loader },
            'css-loader',
            {
              loader: 'stylus-loader',
              options: {
                use: [require('nib')()],
                import: ['~nib/lib/nib/index.styl', path.resolve(__dirname, 'styles/reset.styl')]
              },
            },
          ]
        },
        {
          test: /\.stylus$/,
          use: [
            env.wc ? {
              loader: 'vue-style-loader',
              options: {
                shadowMode: true
              }
            } : {
              loader: MiniCssExtractPlugin.loader,
              options: {
                hmr: !env.production
              }
            },
            'css-loader',
            {
              loader: 'stylus-loader',
              options: {
                use: [require('nib')()],
                import: env.wc ?
                  ['~nib/lib/nib/index.styl', path.resolve(__dirname, 'styles/reset.styl')] :
                  ['~nib/lib/nib/index.styl']
              },
            },
          ]
        }
      ]
    }
  };
};
