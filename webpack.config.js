const path = require('path');
const CopyPlugin = require('copy-webpack-plugin');
const WasmPackPlugin = require('@wasm-tool/wasm-pack-plugin');
const MiniCssExtractPlugin = require('mini-css-extract-plugin');
const VueLoaderPlugin = require('vue-loader/lib/plugin');

const dist = path.resolve(__dirname, 'dist');
const static = path.resolve(__dirname, 'static');

module.exports = (env) => {
  return {
    entry: {
      main: env.wc ?
        ['./js/main-wc.js', './styles/main.styl'] :
        ['./js/main-app.js', './styles/main.styl'],
      // website: ['./js/main-website.js', './styles/website.styl']
    },
    devtool: env.production ? false : '#source-map',
    output: {
      path: dist,
      filename: '[name].js'
    },
    devServer: {
      openPage: 'index.html',
      contentBase: static,
      watchContentBase: true,
      overlay: true,
      hot: true,
    },
    plugins: [
      new MiniCssExtractPlugin(),
      new VueLoaderPlugin(),
      new CopyPlugin(env.wc ? [static] : [
        './package.json',
        static,
        path.resolve(__dirname, 'electron')
      ]),
      new WasmPackPlugin({
        crateDirectory: path.resolve(__dirname, 'rust'),
        watchDirectories: [
          path.resolve(__dirname, 'rust/lib/shapex/src/'),
          path.resolve(__dirname, 'rust/lib/solvo/src/'),
        ],
        extraArgs: '--out-name wasm-index',
      }),
    ],
    module: {
      rules: [
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
            'css-loader?url=false',
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
            'css-loader?url=false',
            {
              loader: 'stylus-loader',
              options: {
                use: [require('nib')()],
                import: env.wc ?
                  ['~nib/lib/nib/index.styl', path.resolve(__dirname, 'styles/reset.styl'), path.resolve(__dirname, 'styles/variables.styl')] :
                  ['~nib/lib/nib/index.styl', path.resolve(__dirname, 'styles/variables.styl')]
              },
            },
          ]
        }
      ]
    }
  };
};
