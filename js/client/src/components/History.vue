<template>
    <b-container>
        <b-row>
            <b-col class="col-auto">
                <b-pagination 
                    v-model="currentPage" 
                    :per-page="rowsPerPage" 
                    :total-rows="totalRows"
                    hide-goto-end-buttons
                ></b-pagination>
            </b-col>
            <b-col class="col-auto">
                <b-button 
                    variant="light"
                    :disabled="loading"
                    @click="loadNewTransactions">Refresh</b-button>
            </b-col>
        </b-row>

        <img v-if="loading" style="margin-right: 1.5em" src="../assets/loading.gif" width="100em">
        <HistoryRow v-else v-for="tx in transactions" :tx="tx" :key="tx.elem_id"></HistoryRow>

        <b-pagination 
            class="mt-3"
            v-model="currentPage" 
            :per-page="rowsPerPage" 
            :total-rows="totalRows" 
            hide-goto-end-buttons
        ></b-pagination>
    </b-container>
</template>

<script>
import timeConstants from '../timeConstants'

import HistoryRow from './HistoryRow.vue'

const components = {
    HistoryRow
};

export default {
    name: 'History',
    data: () => ({
        currentPage: 1,
        rowsPerPage: 10,
        totalRows: 0,

        transactions: [],
        pagesOfTransactions: {},

        intervalHandle: null,
        loading: true,
    }),
    async created() {
        await this.load();
    },
    watch: {
        currentPage: function() {
            this.load();
        },
    },
    methods: {
        async load() {
            this.loading = true;


            let offset = (this.currentPage - 1) * this.rowsPerPage;
            let limit = this.rowsPerPage;

            // maybe load the requested page
            if (this.pagesOfTransactions[this.currentPage] == undefined)
                this.pagesOfTransactions[this.currentPage] 
                    = await window.walletDecorator.transactionsAsRenderableList(offset, limit);
            

            let numNextPageTransactions = null;
            
            // maybe load the next page
            if (this.pagesOfTransactions[this.currentPage + 1] == undefined) {
                let txs = await window.walletDecorator.transactionsAsRenderableList(offset + limit, limit);
                numNextPageTransactions = txs.length;
                // Once we assign txs to pagesOfTransactions,
                // it gets wrapped in vue watchers and stuff.
                // 
                // Sometimes this.pagesOfTransactions[this.currentPage + 1].length
                // is > limit, which I can only explain by vue's wrapping.
                // Hopefully, this will fix it.
                this.pagesOfTransactions[this.currentPage + 1] = txs;
            }

            if (numNextPageTransactions !== null) {
                // we now know if we can add a new page button
                this.totalRows = offset + limit + numNextPageTransactions;
            }

            // display the page
            this.transactions = this.pagesOfTransactions[this.currentPage];

            this.loading = false;
        },
        loadNewTransactions() {
            this.totalRows = 0;
            this.pagesOfTransactions = {};
            this.load();
        },
    },
    components,
}
</script>
